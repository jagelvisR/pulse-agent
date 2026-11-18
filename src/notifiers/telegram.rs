use super::Notifier;
use crate::state::{AlertEvent, Severity};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

pub struct TelegramNotifier {
    client: Client,
    bot_token: String,
    chat_id: String,
}

impl TelegramNotifier {
    pub fn new(bot_token: String, chat_id: String) -> Self {
        Self {
            client: Client::new(),
            bot_token,
            chat_id,
        }
    }
}

#[async_trait]
impl Notifier for TelegramNotifier {
    async fn send(&self, hostname: &str, event: &AlertEvent) -> Result<()> {
        let icon = match event.severity {
            Severity::Critical => "🔴",
            Severity::Warning => "🟡",
            Severity::Resolved => "🟢",
        };

        // Limpiamos el doble asterisco de Discord para compatibilidad con HTML de Telegram
        let clean_desc = event
            .description
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        let text = format!(
            "{} <b>[{}] {}</b>\n{}",
            icon, hostname, event.title, clean_desc
        );

        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let payload = json!({
            "chat_id": self.chat_id,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true
        });

        self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}