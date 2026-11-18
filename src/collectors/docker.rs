use crate::config::DockerConfig;
use crate::metrics::{ContainerInfo, TelemetrySnapshot};
use crate::state::{AlertEvent, Severity, StateManager};
use bollard::container::ListContainersOptions;
use bollard::Docker;
use tracing::error;

pub struct DockerCollector {
    client: Option<Docker>,
}

impl DockerCollector {
    pub fn new(socket_path: &str) -> Self {
        let client_res = if socket_path.starts_with("tcp://") || socket_path.starts_with("http://") {
            Docker::connect_with_http(socket_path, 5, bollard::API_DEFAULT_VERSION)
        } else {
            Docker::connect_with_local_defaults()
        };

        let client = client_res
            .map_err(|e| error!("No se pudo inicializar cliente Docker: {:?}", e))
            .ok();

        Self { client }
    }

    pub async fn check(
        &self,
        cfg: &DockerConfig,
        state: &mut StateManager,
        snapshot: &mut TelemetrySnapshot,
    ) -> Vec<AlertEvent> {
        let mut events = Vec::new();
        snapshot.containers.clear();

        if !cfg.enabled {
            return events;
        }

        let Some(docker) = &self.client else {
            return events;
        };

        let opts = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });

        let Ok(containers) = docker.list_containers(opts).await else {
            error!("Fallo al listar contenedores Docker");
            return events;
        };

        for container in containers {
            let name = container
                .names
                .as_ref()
                .and_then(|n| n.first())
                .map(|s| s.trim_start_matches('/').to_string())
                .unwrap_or_else(|| "unknown".into());

            if !cfg.watch_containers.is_empty() && !cfg.watch_containers.contains(&name) {
                continue;
            }

            let state_str = container.state.unwrap_or_default();
            let status_str = container.status.unwrap_or_default();

            snapshot.containers.push(ContainerInfo {
                name: name.clone(),
                state: state_str.clone(),
                status: status_str.clone(),
            });

            let is_down = state_str != "running";
            let is_unhealthy = cfg.alert_on_unhealthy && status_str.contains("(unhealthy)");
            let is_firing = is_down || is_unhealthy;

            let key = format!("docker:container:{}", name);
            if let Some(ev) = state.evaluate(
                &key,
                is_firing,
                format!("Contenedor Docker: {}", name),
                format!("Estado: `{}` | Detalle: `{}`", state_str, status_str),
                Severity::Critical,
            ) {
                events.push(ev);
            }
        }

        events
    }
}