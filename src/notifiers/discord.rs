use super::Notifier;
use crate::state::{AlertEvent, Severity};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

pub struct DiscordNotifier {
    client: Client,
    webhook_url: String,
}

impl DiscordNotifier {
    pub fn new(webhook_url: String) -> Self {
        Self {
            client: Client::new(),
            webhook_url,
        }
    }
}

#[async_trait]
impl Notifier for DiscordNotifier {
    async fn send(&self, hostname: &str, event: &AlertEvent) -> Result<()> {
        let color = match event.severity {
            Severity::Critical => 0xE74C3C, // Rojo
            Severity::Warning => 0xF39C12,  // Ámbar
            Severity::Resolved => 0x2ECC71, // Verde
        };

        let payload = json!({
            "embeds": [{
                "title": format!("[{}] {}", hostname, event.title),
                "description": event.description,
                "color": color,
                "footer": { "text": "pulse-agent" }
            }]
        });

        self.client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}