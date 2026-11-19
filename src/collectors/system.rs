use crate::config::SystemConfig;
use crate::metrics::ProcessInfo;
use crate::state::{AlertEvent, Severity};
use sysinfo::{Components, Disks, ProcessesToUpdate, System};

pub struct SystemMetrics {
    pub cpu_percent: f32,
    pub ram_percent: f32,
    pub max_temp_c: f32,
    pub disks: Vec<(String, f32)>,
    pub top_processes: Vec<ProcessInfo>,
}

pub struct SystemCollector {
    sys: System,
    disks: Disks,
    components: Components,
}

impl SystemCollector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_cpu_usage();
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
        }
    }

    pub fn collect(&mut self, cfg: &SystemConfig) -> (SystemMetrics, Vec<AlertEvent>) {
        let mut events = Vec::new();

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.disks.refresh();
        self.components.refresh();

        let cpu_percent = self.sys.global_cpu_usage();
        let total_mem = self.sys.total_memory().max(1) as f32;
        let used_mem = self.sys.used_memory() as f32;
        let ram_percent = (used_mem / total_mem) * 100.0;

        let max_temp_c = self
            .components
            .iter()
            .map(|c| c.temperature())
            .filter(|t| t.is_finite() && *t > 0.0)
            .fold(0.0_f32, f32::max);

        let mut disk_list = Vec::new();
        for disk in self.disks.list() {
            let total = disk.total_space();
            if total == 0 {
                continue;
            }
            let mount = disk.mount_point().to_string_lossy().into_owned();
            if mount.starts_with("/snap") || mount.starts_with("/boot") {
                continue;
            }
            let used = total.saturating_sub(disk.available_space());
            let used_pct = (used as f32 / total as f32) * 100.0;
            disk_list.push((mount, used_pct));
        }

        let num_cpus = self.sys.cpus().len().max(1) as f32;
        let mut procs: Vec<ProcessInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, proc)| ProcessInfo {
                pid: pid.as_u32(),
                name: proc.name().to_string_lossy().into_owned(),
                cpu_percent: proc.cpu_usage() / num_cpus,
                ram_mb: proc.memory() / 1024 / 1024,
            })
            .collect();

        procs.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if cfg.enabled {
            events.push(AlertEvent {
                key: "sys:cpu".into(),
                title: "Uso elevado de CPU".into(),
                description: format!("CPU global en {:.1}% (umbral: {:.1}%)", cpu_percent, cfg.cpu_threshold_percent),
                severity: if cpu_percent > cfg.cpu_threshold_percent {
                    Severity::Warning
                } else {
                    Severity::Resolved
                },
            });

            events.push(AlertEvent {
                key: "sys:ram".into(),
                title: "Uso elevado de Memoria RAM".into(),
                description: format!("RAM en {:.1}% (umbral: {:.1}%)", ram_percent, cfg.ram_threshold_percent),
                severity: if ram_percent > cfg.ram_threshold_percent {
                    Severity::Warning
                } else {
                    Severity::Resolved
                },
            });

            if max_temp_c > 0.0 {
                events.push(AlertEvent {
                    key: "sys:temp".into(),
                    title: "Temperatura elevada del sistema".into(),
                    description: format!("Temperatura máxima: {:.1}°C (umbral: {:.1}°C)", max_temp_c, cfg.temp_threshold_celsius),
                    severity: if max_temp_c > cfg.temp_threshold_celsius {
                        Severity::Critical
                    } else {
                        Severity::Resolved
                    },
                });
            }

            for (mount, used_pct) in &disk_list {
                events.push(AlertEvent {
                    key: format!("sys:disk:{}", mount),
                    title: format!("Espacio en disco ({})", mount),
                    description: format!("Partición {} al {:.1}% (umbral: {:.1}%)", mount, used_pct, cfg.disk_threshold_percent),
                    severity: if *used_pct > cfg.disk_threshold_percent {
                        Severity::Warning
                    } else {
                        Severity::Resolved
                    },
                });
            }

            for p in procs.iter().take(15) {
                let over_cpu = p.cpu_percent > cfg.process_cpu_threshold_percent;
                let over_ram = p.ram_mb > cfg.process_ram_threshold_mb;
                if over_cpu || over_ram {
                    events.push(AlertEvent {
                        key: format!("sys:proc:{}:{}", p.name, p.pid),
                        title: format!("Proceso pesado detectado: {}", p.name),
                        description: format!(
                            "PID {} consumiendo {:.1}% CPU y {} MB RAM",
                            p.pid, p.cpu_percent, p.ram_mb
                        ),
                        severity: Severity::Warning,
                    });
                }
            }
        }

        let top_processes = procs.into_iter().take(10).collect();

        (
            SystemMetrics {
                cpu_percent,
                ram_percent,
                max_temp_c,
                disks: disk_list,
                top_processes,
            },
            events,
        )
    }
}