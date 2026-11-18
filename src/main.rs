mod collectors;
mod config;
mod metrics;
mod notifiers;
mod state;
mod tui;

use anyhow::Result;
use clap::Parser;
use collectors::{docker::DockerCollector, http::HttpCollector, system::SystemCollector};
use config::Config;
use metrics::{new_shared_metrics, SharedMetrics};
use notifiers::{discord::DiscordNotifier, telegram::TelegramNotifier, Notifier};
use state::StateManager;
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    // Solo activar logs en stdout si NO estamos en modo TUI (para no ensuciar la pantalla)
    if !cli.ui {
        tracing_subscriber::fmt()
            .with_env_filter("pulse_agent=info")
            .init();

        info!(
            "Iniciando pulse-agent en '{}' (intervalo: {}s, cooldown: {}m)",
            cfg.general.hostname, cfg.general.check_interval_secs, cfg.general.alert_cooldown_mins
        );
    }

    let shared_metrics = new_shared_metrics();

    if cli.ui {
        // En modo UI refrescamos cada 2s para gráficas fluidas
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
    let mut notifiers: Vec<Box<dyn Notifier>> = Vec::new();

    if let Some(discord_cfg) = &cfg.notifiers.discord {
        if discord_cfg.enabled {
            notifiers.push(Box::new(DiscordNotifier::new(
                discord_cfg.webhook_url.clone(),
            )));
        }
    }

    if let Some(tg_cfg) = &cfg.notifiers.telegram {
        if tg_cfg.enabled {
            notifiers.push(Box::new(TelegramNotifier::new(
                tg_cfg.bot_token.clone(),
                tg_cfg.chat_id.clone(),
            )));
        }
    }

    if notifiers.is_empty() && override_interval_secs.is_none() {
        warn!("No hay ningún notificador habilitado en pulse.toml.");
    }

    let mut state = StateManager::new(cfg.general.alert_cooldown_mins);
    let mut sys_collector = SystemCollector::new();
    let docker_collector = DockerCollector::new(&cfg.docker.socket);
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