use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub general: GeneralConfig,
    pub system: SystemConfig,
    pub docker: DockerConfig,
    #[serde(default)]
    pub http_checks: Vec<HttpCheckConfig>,
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct HttpCheckConfig {
    pub name: String,
    pub url: String,
    #[serde(default = "default_status")]
    pub expected_status: u16,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
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

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("No se pudo leer el archivo de configuración: {:?}", path.as_ref()))?;
        let config: Config = toml::from_str(&content)
            .context("Error de sintaxis en el archivo TOML")?;
        Ok(config)
    }
}