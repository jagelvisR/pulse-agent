use crate::config::HttpCheckConfig;
use crate::metrics::{HttpCheckInfo, TelemetrySnapshot};
use crate::state::{AlertEvent, Severity, StateManager};
use reqwest::Client;
use rustls_pki_types::ServerName;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;
use url::Url;
use x509_parser::prelude::*;

pub struct HttpCollector {
    client: Client,
    tls_connector: TlsConnector,
}

impl HttpCollector {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("pulse-agent/0.1")
            .build()
            .unwrap_or_default();

        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let tls_config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Self {
            client,
            tls_connector: TlsConnector::from(Arc::new(tls_config)),
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

        for chk in checks {
            let start = Instant::now();
            let res = self
                .client
                .get(&chk.url)
                .timeout(Duration::from_secs(chk.timeout_secs))
                .send()
                .await;

            let elapsed_ms = start.elapsed().as_millis();

            let (is_firing, detail_msg, status_label) = match res {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status != chk.expected_status {
                        (
                            true,
                            format!(
                                "Código HTTP inesperado: **{}** (esperado: `{}`)",
                                status, chk.expected_status
                            ),
                            format!("ERR {}", status),
                        )
                    } else {
                        (false, "Endpoint respondiendo correctamente".into(), format!("{} OK", status))
                    }
                }
                Err(e) => (
                    true,
                    format!("Fallo de conexión o timeout ({}s): `{}`", chk.timeout_secs, e),
                    "DOWN".into(),
                ),
            };

            // Verificar vencimiento SSL/TLS si el endpoint es HTTPS y está habilitado
            let mut final_label = status_label;
            if chk.url.starts_with("https://") && chk.ssl_expiry_warning_days > 0 {
                if let Some(days_left) = self.check_ssl_days_remaining(&chk.url, chk.timeout_secs).await {
                    final_label = format!("{} · SSL {}d", final_label, days_left);

                    let ssl_firing = days_left <= chk.ssl_expiry_warning_days;
                    let ssl_severity = if days_left <= 3 {
                        Severity::Critical
                    } else {
                        Severity::Warning
                    };
                    let ssl_msg = if days_left < 0 {
                        format!(
                            "El certificado SSL de `{}` **expiró hace {} día(s)**.",
                            chk.url,
                            days_left.abs()
                        )
                    } else {
                        format!(
                            "El certificado SSL de `{}` vence en **{} día(s)** (Umbral: {} días).",
                            chk.url, days_left, chk.ssl_expiry_warning_days
                        )
                    };

                    let ssl_key = format!("ssl:{}", chk.name);
                    if let Some(ev) = state.evaluate(
                        &ssl_key,
                        ssl_firing,
                        format!("Certificado SSL: {}", chk.name),
                        ssl_msg,
                        ssl_severity,
                    ) {
                        events.push(ev);
                    }
                }
            }

            snapshot.http_checks.push(HttpCheckInfo {
                name: chk.name.clone(),
                url: chk.url.clone(),
                ok: !is_firing,
                status_label: final_label,
                latency_ms: elapsed_ms,
            });

            let key = format!("http:{}", chk.name);
            if let Some(ev) = state.evaluate(
                &key,
                is_firing,
                format!("HTTP Check: {}", chk.name),
                format!("URL: `{}` | {}", chk.url, detail_msg),
                Severity::Critical,
            ) {
                events.push(ev);
            }
        }

        events
    }

    async fn check_ssl_days_remaining(&self, raw_url: &str, timeout_secs: u64) -> Option<i64> {
        let parsed = Url::parse(raw_url).ok()?;
        let host = parsed.host_str()?.to_string();
        let port = parsed.port_or_known_default().unwrap_or(443);
        let addr = format!("{}:{}", host, port);

        let tcp = timeout(Duration::from_secs(timeout_secs), TcpStream::connect(&addr))
            .await
            .ok()?
            .ok()?;

        let server_name = ServerName::try_from(host).ok()?;
        let tls_stream = timeout(
            Duration::from_secs(timeout_secs),
            self.tls_connector.connect(server_name, tcp),
        )
            .await
            .ok()?
            .ok()?;

        let (_, conn) = tls_stream.get_ref();
        let certs = conn.peer_certificates()?;
        let leaf = certs.first()?;

        let (_, x509) = X509Certificate::from_der(leaf.as_ref()).ok()?;
        let not_after = x509.validity().not_after.timestamp();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;

        Some((not_after - now) / 86_400)
    }
}