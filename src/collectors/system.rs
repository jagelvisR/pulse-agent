use crate::config::SystemConfig;
use crate::metrics::{DiskInfo, ProcessInfo, TelemetrySnapshot};
use crate::state::{AlertEvent, Severity, StateManager};
use sysinfo::{Components, Disks, ProcessesToUpdate, System};

pub struct SystemCollector {
    sys: System,
    disks: Disks,
    components: Components,
}

impl SystemCollector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
        }
    }

    pub fn check(
        &mut self,
        cfg: &SystemConfig,
        state: &mut StateManager,
        snapshot: &mut TelemetrySnapshot,
    ) -> Vec<AlertEvent> {
        let mut events = Vec::new();
        if !cfg.enabled {
            return events;
        }

        // 1. CPU Global
        self.sys.refresh_cpu_usage();
        let cpu_usage = self.sys.global_cpu_usage();
        if let Some(ev) = state.evaluate(
            "sys:cpu",
            cpu_usage > cfg.cpu_threshold_percent,
            "Uso elevado de CPU".into(),
            format!("CPU actual: **{:.1}%** (Umbral: {:.1}%)", cpu_usage, cfg.cpu_threshold_percent),
            Severity::Warning,
        ) {
            events.push(ev);
        }

        // 2. RAM Global
        self.sys.refresh_memory();
        let total_mem = self.sys.total_memory() as f32;
        let used_mem = self.sys.used_memory() as f32;
        let ram_pct = if total_mem > 0.0 { (used_mem / total_mem) * 100.0 } else { 0.0 };
        if let Some(ev) = state.evaluate(
            "sys:ram",
            ram_pct > cfg.ram_threshold_percent,
            "Memoria RAM crítica".into(),
            format!("Uso de RAM: **{:.1}%** (Umbral: {:.1}%)", ram_pct, cfg.ram_threshold_percent),
            Severity::Critical,
        ) {
            events.push(ev);
        }

        // 3. Procesos individuales del SO
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        let num_cpus = self.sys.cpus().len().max(1) as f32;

        let mut procs: Vec<ProcessInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, proc_data)| {
                let norm_cpu = proc_data.cpu_usage() / num_cpus;
                let ram_mb = proc_data.memory() / (1024 * 1024);
                ProcessInfo {
                    pid: pid.as_u32(),
                    name: proc_data.name().to_string_lossy().to_string(),
                    cpu_percent: norm_cpu,
                    ram_mb,
                }
            })
            .collect();

        // Ordenar por consumo de CPU descendente y luego RAM
        procs.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.ram_mb.cmp(&a.ram_mb))
        });

        // Evaluar alertas sobre los 15 procesos más pesados
        for p in procs.iter().take(15) {
            // Ignorar el proceso inactivo de Windows ("System Idle Process")
            if p.pid == 0 || p.name.eq_ignore_ascii_case("System Idle Process") {
                continue;
            }

            let high_cpu = p.cpu_percent > cfg.process_cpu_threshold_percent;
            let high_ram = p.ram_mb > cfg.process_ram_threshold_mb;
            let key = format!("sys:proc:{}:{}", p.name, p.pid);

            if let Some(ev) = state.evaluate(
                &key,
                high_cpu || high_ram,
                format!("Proceso pesado: {} (PID {})", p.name, p.pid),
                format!(
                    "CPU: **{:.1}%** (Umbral: {:.1}%) | RAM: **{} MB** (Umbral: {} MB)",
                    p.cpu_percent, cfg.process_cpu_threshold_percent, p.ram_mb, cfg.process_ram_threshold_mb
                ),
                Severity::Warning,
            ) {
                events.push(ev);
            }
        }

        let top_processes: Vec<ProcessInfo> = procs.into_iter().take(10).collect();

        // 4. Discos
        self.disks.refresh();
        let mut disk_infos = Vec::new();
        for disk in &self.disks {
            let mount = disk.mount_point().to_string_lossy().to_string();
            if mount.starts_with("/snap") || mount.starts_with("/run") {
                continue;
            }
            let total = disk.total_space() as f32;
            let avail = disk.available_space() as f32;
            if total > 0.0 {
                let used_pct = ((total - avail) / total) * 100.0;
                disk_infos.push(DiskInfo {
                    mount: mount.clone(),
                    used_percent: used_pct,
                });

                let key = format!("sys:disk:{}", mount);
                if let Some(ev) = state.evaluate(
                    &key,
                    used_pct > cfg.disk_threshold_percent,
                    format!("Espacio en disco bajo ({})", mount),
                    format!("Partición `{}` al **{:.1}%**", mount, used_pct),
                    Severity::Critical,
                ) {
                    events.push(ev);
                }
            }
        }

        // 5. Temperaturas
        self.components.refresh();
        let max_temp = self
            .components
            .iter()
            .map(|c| c.temperature())
            .fold(0.0_f32, f32::max);

        if max_temp > 0.0 {
            if let Some(ev) = state.evaluate(
                "sys:temp",
                max_temp > cfg.temp_threshold_celsius,
                "Temperatura de hardware elevada".into(),
                format!("Sensor máximo: **{:.1}°C** (Umbral: {:.1}°C)", max_temp, cfg.temp_threshold_celsius),
                Severity::Critical,
            ) {
                events.push(ev);
            }
        }

        snapshot.push_system_sample(cpu_usage, ram_pct, max_temp, disk_infos, top_processes);
        events
    }
}