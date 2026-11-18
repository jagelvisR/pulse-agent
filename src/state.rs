use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    Critical,
    Warning,
    Resolved,
}

#[derive(Debug, Clone)]
pub struct AlertEvent {
    pub key: String,
    pub title: String,
    pub description: String,
    pub severity: Severity,
}

pub struct StateManager {
    active_alerts: HashMap<String, Instant>,
    cooldown: Duration,
}

impl StateManager {
    pub fn new(cooldown_mins: u64) -> Self {
        Self {
            active_alerts: HashMap::new(),
            cooldown: Duration::from_secs(cooldown_mins * 60),
        }
    }

    /// Evalúa una condición. Retorna Some(AlertEvent) solo si debe notificarse
    /// (nueva alerta, cooldown expirado o recuperación).
    pub fn evaluate(
        &mut self,
        key: &str,
        is_firing: bool,
        title: String,
        description: String,
        severity: Severity,
    ) -> Option<AlertEvent> {
        let now = Instant::now();

        if is_firing {
            match self.active_alerts.get(key) {
                Some(&last_sent) if now.duration_since(last_sent) < self.cooldown => None,
                _ => {
                    self.active_alerts.insert(key.to_string(), now);
                    Some(AlertEvent {
                        key: key.to_string(),
                        title,
                        description,
                        severity,
                    })
                }
            }
        } else if self.active_alerts.remove(key).is_some() {
            Some(AlertEvent {
                key: key.to_string(),
                title: format!("Recuperado: {}", title),
                description,
                severity: Severity::Resolved,
            })
        } else {
            None
        }
    }
}