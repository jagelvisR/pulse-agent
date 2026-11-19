use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_HISTORY: usize = 60;

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub name: String,
    pub state: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct HttpCheckInfo {
    pub name: String,
    pub url: String,
    pub ok: bool,
    pub status_label: String,
    pub latency_ms: u128,
}

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub mount: String,
    pub used_percent: f32,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub ram_mb: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TelemetrySnapshot {
    pub cpu_percent: f32,
    pub ram_percent: f32,
    pub max_temp_c: f32,
    pub cpu_history: VecDeque<f64>,
    pub ram_history: VecDeque<f64>,
    pub disks: Vec<DiskInfo>,
    pub top_processes: Vec<ProcessInfo>,
    pub containers: Vec<ContainerInfo>,
    pub http_checks: Vec<HttpCheckInfo>,
    pub recent_logs: VecDeque<String>,
}

impl TelemetrySnapshot {
    pub fn push_system_sample(
        &mut self,
        cpu: f32,
        ram: f32,
        temp: f32,
        disks: Vec<DiskInfo>,
        top_processes: Vec<ProcessInfo>,
    ) {
        self.cpu_percent = cpu;
        self.ram_percent = ram;
        self.max_temp_c = temp;
        self.disks = disks;
        self.top_processes = top_processes;

        if self.cpu_history.len() >= MAX_HISTORY {
            self.cpu_history.pop_front();
        }
        self.cpu_history.push_back(cpu as f64);

        if self.ram_history.len() >= MAX_HISTORY {
            self.ram_history.pop_front();
        }
        self.ram_history.push_back(ram as f64);
    }

    pub fn push_log(&mut self, msg: String) {
        if self.recent_logs.len() >= 100 {
            self.recent_logs.pop_front();
        }
        self.recent_logs.push_back(msg);
    }
}

pub type SharedMetrics = Arc<RwLock<TelemetrySnapshot>>;

pub fn new_shared_metrics() -> SharedMetrics {
    Arc::new(RwLock::new(TelemetrySnapshot::default()))
}