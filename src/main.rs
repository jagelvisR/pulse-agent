mod collectors;
mod config;
mod metrics;
mod notifiers;
mod state;
mod tui;

use anyhow::Result;
use clap::Parser;
use collectors::{docker::DockerCollector, http::HttpCollector, system::SystemCollector};
use config::{Config, HeartbeatConfig};
use metrics::{new_shared_metrics, DiskInfo, SharedMetrics};
use notifiers::{discord::DiscordNotifier, telegram::TelegramNotifier, Notifier};
use state::{AlertEvent, Severity, StateManager};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(
    name = "pulse-agent",
    version,
    about = "Agente ultraligero de telemetría, monitoreo TUI y alertas instantáneas"
)]
struct Cli {
    /// Ruta al archivo de configuración TOML
    #[arg(short, long, default_value = "pulse.toml")]
    config: PathBuf,

    /// Abre la interfaz gráfica interactiva en consola (TUI)
    #[arg(long, default_value_t = false)]
    ui: bool,

    /// Envía una alerta de prueba a los canales configurados y sale
    #[arg(long, default_value_t = false)]
    test_alert: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.ui {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    }

    let cfg = Config::load(&cli.config)?;
    let notifiers = build_notifiers(&cfg);

    if cli.test_alert {
        return run_test_alert(&cfg, &notifiers).await;
    }

    let shared_metrics: SharedMetrics = new_shared_metrics();

    if let Some(hb_cfg) = cfg.heartbeat.clone() {
        if hb_cfg.enabled {
            let hostname = cfg.general.hostname.clone();
            tokio::spawn(async move {
                run_heartbeat_loop(hb_cfg, hostname).await;
            });
        }
    }

    if cli.ui {
        let bg_cfg = cfg.clone();
        let bg_metrics = Arc::clone(&shared_metrics);
        tokio::spawn(async move {
            let _ =
                run_monitor_loop(bg_cfg, notifiers, bg_metrics, Duration::from_secs(2), true).await;
        });

        tui::run_tui(cfg.general.hostname, shared_metrics).await?;
    } else {
        info!(
            hostname = %cfg.general.hostname,
            interval_secs = cfg.general.check_interval_secs,
            "Iniciando pulse-agent en modo demonio"
        );
        let interval = Duration::from_secs(cfg.general.check_interval_secs.max(5));
        tokio::select! {
            res = run_monitor_loop(cfg, notifiers, shared_metrics, interval, false) => res?,
            _ = tokio::signal::ctrl_c() => {
                info!("Señal de detención recibida, cerrando pulse-agent...");
            }
        }
    }

    Ok(())
}

fn build_notifiers(cfg: &Config) -> Vec<Box<dyn Notifier>> {
    let mut list: Vec<Box<dyn Notifier>> = Vec::new();

    if let Some(discord) = &cfg.notifiers.discord {
        if discord.enabled && !discord.webhook_url.is_empty() {
            list.push(Box::new(DiscordNotifier::new(discord.webhook_url.clone())));
        }
    }

    if let Some(telegram) = &cfg.notifiers.telegram {
        if telegram.enabled
            && !telegram.bot_token.is_empty()
            && !telegram.chat_id.is_empty()
        {
            list.push(Box::new(TelegramNotifier::new(
                telegram.bot_token.clone(),
                telegram.chat_id.clone(),
            )));
        }
    }

    list
}

async fn run_test_alert(cfg: &Config, notifiers: &[Box<dyn Notifier>]) -> Result<()> {
    if notifiers.is_empty() {
        anyhow::bail!(
            "No hay notificadores habilitados (revisa Discord/Telegram en pulse.toml o variables PULSE_*)"
        );
    }

    let test_event = AlertEvent {
        key: "test:alert".into(),
        title: "Prueba de conectividad de pulse-agent".into(),
        description: format!(
            "Notificación de prueba enviada correctamente desde '{}'.",
            cfg.general.hostname
        ),
        severity: Severity::Resolved,
    };

    for notifier in notifiers {
        notifier.send(&cfg.general.hostname, &test_event).await?;
    }

    println!(
        "✅ Alerta de prueba enviada correctamente a {} canal(es).",
        notifiers.len()
    );
    Ok(())
}

async fn run_heartbeat_loop(hb: HeartbeatConfig, hostname: String) {
    if hb.url.trim().is_empty() {
        warn!("Heartbeat habilitado pero sin URL configurada");
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let mut ticker = tokio::time::interval(Duration::from_secs(hb.interval_secs.max(10)));
    loop {
        ticker.tick().await;
        match client.get(&hb.url).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(hostname = %hostname, "Heartbeat ping enviado correctamente");
            }
            Ok(resp) => {
                warn!(status = %resp.status(), "Heartbeat respondió con código inesperado");
            }
            Err(err) => {
                warn!(error = %err, "Error enviando ping de Heartbeat");
            }
        }
    }
}

async fn run_monitor_loop(
    cfg: Config,
    notifiers: Vec<Box<dyn Notifier>>,
    shared_metrics: SharedMetrics,
    interval: Duration,
    is_ui: bool,
) -> Result<()> {
    let mut sys_collector = SystemCollector::new();
    let mut docker_collector = DockerCollector::new(&cfg.docker.socket);
    let http_collector = HttpCollector::new();
    let mut state_mgr = StateManager::new(cfg.general.alert_cooldown_mins);
    let mut ticker = tokio::time::interval(interval);

    loop {
        ticker.tick().await;

        let (sys_metrics, sys_raw_events) = sys_collector.collect(&cfg.system);
        let mut all_alerts = Vec::new();

        for ev in sys_raw_events {
            let is_firing = !matches!(ev.severity, Severity::Resolved);
            if let Some(alert) =
                state_mgr.evaluate(&ev.key, is_firing, ev.title, ev.description, ev.severity)
            {
                all_alerts.push(alert);
            }
        }

        {
            let mut snap = shared_metrics.write().await;
            let disks: Vec<DiskInfo> = sys_metrics
                .disks
                .into_iter()
                .map(|(mount, used_percent)| DiskInfo {
                    mount,
                    used_percent,
                })
                .collect();

            snap.push_system_sample(
                sys_metrics.cpu_percent,
                sys_metrics.ram_percent,
                sys_metrics.max_temp_c,
                disks,
                sys_metrics.top_processes,
            );

            let docker_alerts = docker_collector
                .check(&cfg.docker, &mut state_mgr, &mut snap)
                .await;
            let http_alerts = http_collector
                .check(&cfg.http_checks, &mut state_mgr, &mut snap)
                .await;

            all_alerts.extend(docker_alerts);
            all_alerts.extend(http_alerts);

            for ev in &all_alerts {
                snap.push_log(format!("[{:?}] {}: {}", ev.severity, ev.title, ev.description));
            }
        }

        for ev in &all_alerts {
            if !is_ui {
                info!(
                    severity = ?ev.severity,
                    title = %ev.title,
                    description = %ev.description,
                    "Evento disparado"
                );
            }
            for notifier in &notifiers {
                if let Err(e) = notifier.send(&cfg.general.hostname, ev).await {
                    error!(error = %e, "Fallo enviando notificación");
                }
            }
        }
    }
}