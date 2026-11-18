use crate::config::HttpCheckConfig;
use crate::metrics::{HttpCheckInfo, TelemetrySnapshot};
use crate::state::{AlertEvent, Severity, StateManager};
use reqwest::Client;
use std::time::{Duration, Instant};

pub struct HttpCollector {
    client: Client,
}

impl HttpCollector {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("pulse-agent/0.1.0")
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub async fn check(
        &self,
        checks: &[HttpCheckConfig],
        state: &mut StateManager,
        snapshot: &mut TelemetrySnapshot,
    ) -> Vec<AlertEvent> {
        let mut events = Vec::new();
        snapshot.http_checks.clear();

        for check in checks {
            let start = Instant::now();
            let res = self
                .client
                .get(&check.url)
                .timeout(Duration::from_secs(check.timeout_secs))
                .send()
                .await;

            let elapsed_ms = start.elapsed().as_millis();
            let key = format!("http:{}", check.name);

            let (is_firing, description, status_label) = match res {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status == check.expected_status {
                        (
                            false,
                            format!("Endpoint `{}` responde **{}** ({} ms)", check.url, status, elapsed_ms),
                            format!("{} OK", status),
                        )
                    } else {
                        (
                            true,
                            format!("Endpoint `{}` devolvió **{}** (Esperado: {})", check.url, status, check.expected_status),
                            format!("ERR {}", status),
                        )
                    }
                }
                Err(e) => (
                    true,
                    format!("Fallo conectando a `{}`: {}", check.url, e),
                    "TIMEOUT/ERR".to_string(),
                ),
            };

            snapshot.http_checks.push(HttpCheckInfo {
                name: check.name.clone(),
                url: check.url.clone(),
                ok: !is_firing,
                status_label,
                latency_ms: elapsed_ms,
            });

            if let Some(ev) = state.evaluate(
                &key,
                is_firing,
                format!("HTTP Check: {}", check.name),
                description,
                Severity::Critical,
            ) {
                events.push(ev);
            }
        }

        events
    }
}