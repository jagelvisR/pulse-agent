mod collectors;
mod config;
mod metrics;
mod notifiers;
mod state;
mod tui;

use anyhow::{bail, Result};
use clap::Parser;
use collectors::{docker::DockerCollector, http::HttpCollector, system::SystemCollector};
use config::{Config, HeartbeatConfig};
use metrics::{new_shared_metrics, SharedMetrics};
use notifiers::{discord::DiscordNotifier, telegram::TelegramNotifier, Notifier};
use state::{AlertEvent, Severity, StateManager};
use std::path::PathBuf;
use std::time::Duration;
use tokio::signal;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(
    name = "pulse-agent",
    version,
    about = "Agente ligero de telemetría con modo consola interactivo (--ui)"
)]
struct Cli {
    /// Ruta al archivo de configuración TOML
    #[arg(short, long, default_value = "pulse.toml")]
    config: PathBuf,

    /// Inicia la interfaz gráfica en consola (TUI)
    #[arg(long, default_value_t = false)]
    ui: bool,

    /// Envía una alerta de prueba a los canales configurados y sale
    #[arg(long, default_value_t = false)]
    test_alert: bool,
}

fn build_notifiers(cfg: &Config) -> Vec<Box<dyn Notifier>> {
    let mut notifiers: Vec<Box<dyn Notifier>> = Vec::new();

    if let Some(discord_cfg) = &cfg.notifiers.discord {
        if discord_cfg.enabled && !discord_cfg.webhook_url.trim().is_empty() {
            notifiers.push(Box::new(DiscordNotifier::new(
                discord_cfg.webhook_url.clone(),
            )));
        }
    }

    if let Some(tg_cfg) = &cfg.notifiers.telegram {
        if tg_cfg.enabled
            && !tg_cfg.bot_token.trim().is_empty()
            && !tg_cfg.chat_id.trim().is_empty()
        {
            notifiers.push(Box::new(TelegramNotifier::new(
                tg_cfg.bot_token.clone(),
                tg_cfg.chat_id.clone(),
            )));
        }
    }

    notifiers
}

async fn run_test_alert(cfg: &Config) -> Result<()> {
    let notifiers = build_notifiers(cfg);
    if notifiers.is_empty() {
        bail!(
            "No hay notificadores habilitados. Configura pulse.toml o define PULSE_DISCORD_WEBHOOK / PULSE_TELEGRAM_TOKEN + PULSE_TELEGRAM_CHAT_ID."
        );
    }

    let test_event = AlertEvent {
        key: "pulse:test".to_string(),
        title: "Notificación de prueba de pulse-agent".to_string(),
        description: format!(
            "Conexión verificada correctamente desde el nodo `{}`.",
            cfg.general.hostname
        ),
        severity: Severity::Resolved,
    };

    info!(
        "Enviando alerta de prueba a {} canal(es)...",
        notifiers.len()
    );

    let mut failed = 0;
    for notifier in &notifiers {
        if let Err(e) = notifier.send(&cfg.general.hostname, &test_event).await {
            error!("Error enviando prueba: {:?}", e);
            failed += 1;
        }
    }

    if failed > 0 {
        bail!("Fallaron {} de {} notificadores.", failed, notifiers.len());
    }

    info!("Alerta de prueba entregada con éxito.");
    Ok(())
}

async fn run_heartbeat_loop(hb: HeartbeatConfig, silent: bool) {
    if !hb.enabled || hb.url.trim().is_empty() {
        return;
    }

    let client = reqwest::Client::builder()
        .user_agent("pulse-agent-heartbeat/0.1")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let mut interval = tokio::time::interval(Duration::from_secs(hb.interval_secs.max(10)));

    loop {
        interval.tick().await;
        if let Err(e) = client.get(&hb.url).send().await {
            if !silent {
                warn!("Fallo al enviar ping de heartbeat a {}: {:?}", hb.url, e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    if !cli.ui {
        tracing_subscriber::fmt()
            .with_env_filter("pulse_agent=info")
            .init();
    }

    if cli.test_alert {
        return run_test_alert(&cfg).await;
    }

    if !cli.ui {
        info!(
            "Iniciando pulse-agent en '{}' (intervalo: {}s, cooldown: {}m)",
            cfg.general.hostname, cfg.general.check_interval_secs, cfg.general.alert_cooldown_mins
        );
    }

    if let Some(hb) = cfg.heartbeat.clone() {
        if hb.enabled && !hb.url.trim().is_empty() {
            if !cli.ui {
                info!("Heartbeat activo cada {}s -> {}", hb.interval_secs, hb.url);
            }
            tokio::spawn(run_heartbeat_loop(hb, cli.ui));
        }
    }

    let shared_metrics = new_shared_metrics();

    if cli.ui {
        let bg_cfg = cfg.clone();
        let bg_metrics = shared_metrics.clone();
        tokio::spawn(async move {
            run_collector_loop(bg_cfg, bg_metrics, Some(2)).await;
        });

        tui::run_tui(cfg.general.hostname.clone(), shared_metrics).await?;
    } else {
        tokio::select! {
            _ = run_collector_loop(cfg, shared_metrics, None) => {}
            _ = signal::ctrl_c() => {
                info!("Señal de detención recibida. Cerrando pulse-agent...");
            }
        }
    }

    Ok(())
}

async fn run_collector_loop(
    cfg: Config,
    metrics: SharedMetrics,
    override_interval_secs: Option<u64>,
) {
    let notifiers = build_notifiers(&cfg);

    if notifiers.is_empty() && override_interval_secs.is_none() {
        warn!("No hay ningún notificador habilitado en pulse.toml ni en variables de entorno.");
    }

    let mut state = StateManager::new(cfg.general.alert_cooldown_mins);
    let mut sys_collector = SystemCollector::new();
    let mut docker_collector = DockerCollector::new(&cfg.docker.socket);
    let http_collector = HttpCollector::new();

    let interval_secs = override_interval_secs.unwrap_or(cfg.general.check_interval_secs);
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

    loop {
        interval.tick().await;

        let mut snap = metrics.write().await;
        let mut events = sys_collector.check(&cfg.system, &mut state, &mut snap);
        events.extend(docker_collector.check(&cfg.docker, &mut state, &mut snap).await);
        events.extend(http_collector.check(&cfg.http_checks, &mut state, &mut snap).await);

        for ev in &events {
            snap.push_log(format!("[{}] {}", ev.key, ev.title));
        }
        drop(snap);

        for ev in events {
            if override_interval_secs.is_none() {
                info!("Alerta [{}]: {} -> {}", ev.key, ev.title, ev.description);
            }
            for notifier in &notifiers {
                if let Err(e) = notifier.send(&cfg.general.hostname, &ev).await {
                    if override_interval_secs.is_none() {
                        error!("Error enviando notificación: {:?}", e);
                    }
                }
            }
        }
    }
}