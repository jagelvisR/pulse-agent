# ⚡ pulse-agent

[![Release](https://img.shields.io/github/v/release/TU_USUARIO/pulse-agent?style=flat-square&color=blue)](https://github.com/TU_USUARIO/pulse-agent/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)

**pulse-agent** is an ultra-lightweight (~8 MB RAM), single-binary telemetry, TUI monitor, and alerting agent written in Rust. Built for homelabs, self-hosted setups, and small VPS instances that need instant alerts and real-time terminal dashboards without the overhead of heavy monitoring stacks.

---

## ✨ Features

- **Minimal Footprint:** Uses < 10 MB of RAM at rest and near 0% CPU.
- **Zero Runtime Dependencies:** Compiles to a single static binary (`musl`).
- **Interactive Terminal UI (`--ui`):** Built-in `htop`/`btop`-style dashboard powered by `ratatui` with live Braille line charts for CPU & RAM, disk gauges, Docker container status tables, HTTP latency metrics, and an event log.
- **System Telemetry:** Monitors CPU usage, RAM, disk partitions, and hardware temperatures.
- **Docker Native:** Connects directly to `/var/run/docker.sock` (or Windows named pipe) to detect stopped or `unhealthy` containers.
- **HTTP Healthchecks:** Periodically probes endpoints for expected HTTP status codes, timeouts, and response latency.
- **Smart Alerting & Cooldown:** Built-in state manager prevents alert spam and automatically sends **Resolved** notifications when services recover.
- **Multi-Channel Notifiers:** Native support for **Discord** (rich embeds) and **Telegram** (HTML formatted alerts).

---

## 🚀 Quick Start

### Option 1: Docker Compose (Recommended for Background Daemon)

1. Download the example configuration:
   ```bash
   curl -o pulse.toml [https://raw.githubusercontent.com/TU_USUARIO/pulse-agent/main/pulse.example.toml](https://raw.githubusercontent.com/TU_USUARIO/pulse-agent/main/pulse.example.toml)
   ```
2. Edit `pulse.toml` with your Discord webhook or Telegram bot credentials.
3. Run with `docker compose up -d` using the `docker-compose.yml` below:

```yaml
services:
  pulse-agent:
    image: ghcr.io/TU_USUARIO/pulse-agent:latest
    container_name: pulse-agent
    restart: unless-stopped
    pid: "host"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
      - ./pulse.toml:/config/pulse.toml:ro
```

### Option 2: Docker Run

```bash
docker run -d \
  --name pulse-agent \
  --restart unless-stopped \
  --pid="host" \
  -v /var/run/docker.sock:/var/run/docker.sock:ro \
  -v $(pwd)/pulse.toml:/config/pulse.toml:ro \
  ghcr.io/TU_USUARIO/pulse-agent:latest
```

### Option 3: Standalone Binary & Interactive TUI (`--ui`)

Download the pre-compiled binary for your architecture from the [Releases](https://github.com/TU_USUARIO/pulse-agent/releases) page:

```bash
chmod +x pulse-agent-linux-x86_64

# Run silently as a background daemon:
./pulse-agent-linux-x86_64 --config pulse.toml

# Or launch the live terminal dashboard (press 'q' to exit):
./pulse-agent-linux-x86_64 --config pulse.toml --ui
```

---

## ⚙️ Configuration (`pulse.toml`)

```toml
[general]
hostname = "prod-vps-01"
check_interval_secs = 30
alert_cooldown_mins = 15

[system]
enabled = true
cpu_threshold_percent = 85.0
ram_threshold_percent = 90.0
disk_threshold_percent = 85.0
temp_threshold_celsius = 78.0

[docker]
enabled = true
socket = "/var/run/docker.sock"
# Leave empty [] to monitor all containers, or specify container names:
watch_containers = ["api-backend", "postgres-db", "nginx-proxy"]
alert_on_unhealthy = true

[[http_checks]]
name = "Main API"
url = "[https://api.example.com/health](https://api.example.com/health)"
expected_status = 200
timeout_secs = 5

[notifiers.discord]
enabled = true
webhook_url = "[https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN](https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN)"

[notifiers.telegram]
enabled = false
bot_token = "123456789:ABCdefGHIjklMNOpqrSTUvwxYZ"
chat_id = "-1001234567890"
```

---

## 🛠️ Building from Source

```bash
git clone [https://github.com/TU_USUARIO/pulse-agent.git](https://github.com/TU_USUARIO/pulse-agent.git)
cd pulse-agent
cargo build --release
```

---

## 📄 License

MIT License — see [LICENSE](LICENSE) for details.