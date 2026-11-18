use crate::config::SystemConfig;
use crate::metrics::{DiskInfo, TelemetrySnapshot};
use crate::state::{AlertEvent, Severity, StateManager};
use sysinfo::{Components, Disks, System};

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

        // 1. CPU
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

        // 2. RAM
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

        // 3. Discos
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

        // 4. Temperaturas
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

        snapshot.push_system_sample(cpu_usage, ram_pct, max_temp, disk_infos);
        events
    }
}