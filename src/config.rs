use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub general: GeneralConfig,
    pub system: SystemConfig,
    pub docker: DockerConfig,
    #[serde(default)]
    pub http_checks: Vec<HttpCheckConfig>,
    #[serde(default)]
    pub heartbeat: Option<HeartbeatConfig>,
    #[serde(default)]
    pub notifiers: NotifiersConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GeneralConfig {
    pub hostname: String,
    #[serde(default = "default_interval")]
    pub check_interval_secs: u64,
    #[serde(default = "default_cooldown")]
    pub alert_cooldown_mins: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SystemConfig {
    pub enabled: bool,
    pub cpu_threshold_percent: f32,
    pub ram_threshold_percent: f32,
    pub disk_threshold_percent: f32,
    pub temp_threshold_celsius: f32,
    #[serde(default = "default_proc_cpu")]
    pub process_cpu_threshold_percent: f32,
    #[serde(default = "default_proc_ram_mb")]
    pub process_ram_threshold_mb: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DockerConfig {
    pub enabled: bool,
    #[serde(default = "default_socket")]
    pub socket: String,
    #[serde(default)]
    pub watch_containers: Vec<String>,
    #[serde(default = "default_true")]
    pub alert_on_unhealthy: bool,
    #[serde(default = "default_restart_threshold")]
    pub restart_delta_threshold: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HttpCheckConfig {
    pub name: String,
    pub url: String,
    #[serde(default = "default_status")]
    pub expected_status: u16,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Alerta cuando al certificado SSL/TLS le queden menos de N días (0 = desactivado)
    #[serde(default = "default_ssl_days")]
    pub ssl_expiry_warning_days: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HeartbeatConfig {
    pub enabled: bool,
    pub url: String,
    #[serde(default = "default_heartbeat_interval")]
    pub interval_secs: u64,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct NotifiersConfig {
    #[serde(default)]
    pub discord: Option<DiscordConfig>,
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub webhook_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub chat_id: String,
}

fn default_interval() -> u64 { 30 }
fn default_cooldown() -> u64 { 15 }
fn default_socket() -> String { "/var/run/docker.sock".to_string() }
fn default_true() -> bool { true }
fn default_status() -> u16 { 200 }
fn default_timeout() -> u64 { 5 }
fn default_proc_cpu() -> f32 { 80.0 }
fn default_proc_ram_mb() -> u64 { 2048 }
fn default_restart_threshold() -> i64 { 1 }
fn default_ssl_days() -> i64 { 14 }
fn default_heartbeat_interval() -> u64 { 300 }

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "No se pudo leer el archivo de configuración: {:?}",
                path.as_ref()
            )
        })?;
        let mut config: Config =
            toml::from_str(&content).context("Error de sintaxis en el archivo TOML")?;

        config.apply_env_overrides();
        Ok(config)
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(host) = env::var("PULSE_HOSTNAME") {
            if !host.trim().is_empty() {
                self.general.hostname = host;
            }
        }

        if let Ok(webhook) = env::var("PULSE_DISCORD_WEBHOOK") {
            let webhook = webhook.trim().to_string();
            if !webhook.is_empty() {
                self.notifiers.discord = Some(DiscordConfig {
                    enabled: true,
                    webhook_url: webhook,
                });
            }
        }

        let env_token = env::var("PULSE_TELEGRAM_TOKEN").ok().filter(|s| !s.trim().is_empty());
        let env_chat = env::var("PULSE_TELEGRAM_CHAT_ID").ok().filter(|s| !s.trim().is_empty());

        if env_token.is_some() || env_chat.is_some() {
            let current = self.notifiers.telegram.clone().unwrap_or(TelegramConfig {
                enabled: true,
                bot_token: String::new(),
                chat_id: String::new(),
            });

            let bot_token = env_token.unwrap_or(current.bot_token);
            let chat_id = env_chat.unwrap_or(current.chat_id);
            let enabled = !bot_token.is_empty() && !chat_id.is_empty();

            self.notifiers.telegram = Some(TelegramConfig {
                enabled,
                bot_token,
                chat_id,
            });
        }

        if let Ok(hb_url) = env::var("PULSE_HEARTBEAT_URL") {
            let hb_url = hb_url.trim().to_string();
            if !hb_url.is_empty() {
                let interval_secs = self
                    .heartbeat
                    .as_ref()
                    .map(|h| h.interval_secs)
                    .unwrap_or_else(default_heartbeat_interval);
                self.heartbeat = Some(HeartbeatConfig {
                    enabled: true,
                    url: hb_url,
                    interval_secs,
                });
            }
        }
    }
}