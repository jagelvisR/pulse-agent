pub mod discord;
pub mod telegram;

use crate::state::AlertEvent;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn send(&self, hostname: &str, event: &AlertEvent) -> Result<()>;
}