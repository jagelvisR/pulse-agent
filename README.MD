# ⚡ pulse-agent

[![Release](https://img.shields.io/github/v/release/jagelvisr/pulse-agent?style=flat-square&color=blue)](https://github.com/jagelvisr/pulse-agent/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)

**pulse-agent** es un agente ultraligero (~8 MB de RAM) de telemetría, monitoreo en consola (TUI) y alertas instantáneas compilado en un único binario estático en Rust. Diseñado para *homelabs*, entornos autoalojados (*self-hosted*) y instancias VPS pequeñas que requieren alertas inmediatas y paneles en tiempo real en terminal sin la sobrecarga de stacks pesados de monitoreo.

---

## ✨ Características

- **Huella Mínima:** Consume < 10 MB de RAM en reposo y cerca del 0% de CPU.
- **Cero Dependencias en Ejecución:** Compila a un único binario estático (`musl`).
- **Interfaz Interactiva en Terminal (`--ui`):** Panel estilo `htop`/`btop` construido con `ratatui` que incluye gráficas de líneas Braille en vivo para CPU y RAM, medidores de disco, estado de contenedores Docker (con contador de reinicios), latencia HTTP con días restantes de SSL, top de procesos del sistema operativo y registro de eventos.
- **Búsqueda en Vivo, Ordenamiento y Navegación Completa:** Filtra cualquier panel en tiempo real con `/`, alterna el ordenamiento de procesos entre CPU y RAM con `s`, cambia de panel con `Tab` o clic izquierdo, y desplázate vertical (`↑/↓` o rueda) y horizontalmente (`←/→` o `Shift + Rueda`).
- **Telemetría de Sistema y Procesos:** Monitorea uso global de CPU, RAM, particiones de disco, temperaturas de hardware y procesos individuales del sistema operativo que superen los umbrales de CPU o RAM configurados.
- **Nativo para Docker:** Se conecta directamente a `/var/run/docker.sock` (Linux) o al *named pipe* de Docker Desktop (Windows) para detectar contenedores detenidos, en estado `unhealthy` o con bucles de reinicio (`RestartCount`), soportando filtrado por prefijo de nombre.
- **Healthchecks HTTP y Vencimiento SSL/TLS:** Sondea periódicamente endpoints verificando códigos de estado HTTP esperados, tiempos límite (*timeouts*), latencia de respuesta y días restantes antes de que caduque el certificado X.509 en URLs `https://`.
- **Heartbeat (*Dead Man's Switch*):** Envía pings periódicos a servicios externos (Healthchecks.io, Uptime Kuma) para avisar si el servidor completo pierde conectividad.
- **Alertas Inteligentes, Cooldown y Secretos por Entorno:** Gestor de estado integrado que evita el spam de notificaciones repetidas, envía avisos automáticos de **Recuperado**, permite probar canales con `--test-alert` y soporta variables de entorno para credenciales.

---

## 🚀 Inicio Rápido

### Opción 1: Docker Compose (Recomendado como Demonio en Segundo Plano)

1. Descarga el archivo de configuración de ejemplo:
   ```bash
   curl -o pulse.toml [https://raw.githubusercontent.com/jagelvisr/pulse-agent/main/pulse.example.toml](https://raw.githubusercontent.com/jagelvisr/pulse-agent/main/pulse.example.toml)
   ```
2. Edita `pulse.toml` o pasa tus credenciales mediante variables de entorno en `docker-compose.yml`:

```yaml
services:
  pulse-agent:
    image: ghcr.io/jagelvisr/pulse-agent:latest
    container_name: pulse-agent
    restart: unless-stopped
    pid: "host"
    environment:
      - PULSE_HOSTNAME=prod-vps-01
      - PULSE_DISCORD_WEBHOOK=[https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN](https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN)
      # - PULSE_TELEGRAM_TOKEN=123456789:ABCdefGHIjklMNOpqrSTUvwxYZ
      # - PULSE_TELEGRAM_CHAT_ID=-1001234567890
      # - PULSE_HEARTBEAT_URL=[https://hc-ping.com/tu-uuid-aqui](https://hc-ping.com/tu-uuid-aqui)
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
      - ./pulse.toml:/config/pulse.toml:ro
```

3. Inicia el contenedor:
   ```bash
   docker compose up -d
   ```

### Opción 2: Docker Run

```bash
docker run -d \
  --name pulse-agent \
  --restart unless-stopped \
  --pid="host" \
  -e PULSE_DISCORD_WEBHOOK="[https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN](https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN)" \
  -v /var/run/docker.sock:/var/run/docker.sock:ro \
  -v $(pwd)/pulse.toml:/config/pulse.toml:ro \
  ghcr.io/jagelvisr/pulse-agent:latest
```

### Opción 3: Binario Independiente, Prueba de Alertas e Interfaz TUI (`--ui`)

Descarga el binario precompilado para tu arquitectura desde la página de [Releases](https://github.com/jagelvisr/pulse-agent/releases):

```bash
chmod +x pulse-agent-linux-x86_64

# Verificar que los webhooks de Discord/Telegram funcionan correctamente:
./pulse-agent-linux-x86_64 --config pulse.toml --test-alert

# Ejecutar silenciosamente como demonio en segundo plano:
./pulse-agent-linux-x86_64 --config pulse.toml

# O abrir el panel interactivo en terminal (presiona 'q' o Ctrl+C para salir):
./pulse-agent-linux-x86_64 --config pulse.toml --ui
```

---

## ⌨️ Controles de la Interfaz TUI (`--ui`)

| Acción | Teclado | Mouse |
| :--- | :--- | :--- |
| **Filtrar paneles en tiempo real** | `/` (escribe filtro, `Enter` aplica, `Esc` limpia) | — |
| **Alternar orden Top SO (CPU / RAM)** | `s` | — |
| **Cambiar panel activo** | `Tab` | Clic izquierdo sobre el panel |
| **Desplazamiento vertical** | `↑` / `↓` o `j` / `k` | Rueda hacia arriba / abajo |
| **Desplazamiento horizontal** | `←` / `→` o `h` / `l` | `Shift` + Rueda del mouse |
| **Salir de la interfaz** | `q` o `Ctrl+C` | — |

---

## 🔐 Variables de Entorno Soportadas

Las variables de entorno sobrescriben los valores de `pulse.toml` y habilitan automáticamente el canal correspondiente:

| Variable | Descripción |
| :--- | :--- |
| `PULSE_HOSTNAME` | Sobrescribe `general.hostname` |
| `PULSE_DISCORD_WEBHOOK` | Define `notifiers.discord.webhook_url` y activa Discord |
| `PULSE_TELEGRAM_TOKEN` | Define `notifiers.telegram.bot_token` |
| `PULSE_TELEGRAM_CHAT_ID` | Define `notifiers.telegram.chat_id` y activa Telegram |
| `PULSE_HEARTBEAT_URL` | Define `heartbeat.url` y activa el ping de Heartbeat |

---

## ⚙️ Configuración (`pulse.toml`)

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
process_cpu_threshold_percent = 80.0
process_ram_threshold_mb = 2048

[docker]
enabled = true
socket = "/var/run/docker.sock"
# Dejar vacío [] para monitorear todos los contenedores, o indicar prefijos/nombres:
watch_containers = []
alert_on_unhealthy = true
restart_delta_threshold = 1

[[http_checks]]
name = "API Principal"
url = "[https://api.interfaceslte.com/health](https://api.interfaceslte.com/health)"
expected_status = 200
timeout_secs = 5
ssl_expiry_warning_days = 14

[heartbeat]
enabled = false
url = "[https://hc-ping.com/tu-uuid-aqui](https://hc-ping.com/tu-uuid-aqui)"
interval_secs = 300

[notifiers.discord]
enabled = true
webhook_url = "[https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN](https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN)"

[notifiers.telegram]
enabled = false
bot_token = "123456789:ABCdefGHIjklMNOpqrSTUvwxYZ"
chat_id = "-1001234567890"
```

---

## 🛠️ Compilar desde el Código Fuente

```bash
git clone [https://github.com/jagelvisr/pulse-agent.git](https://github.com/jagelvisr/pulse-agent.git)
cd pulse-agent
cargo build --release
```

---

## 📄 Licencia

Licencia MIT — consulta el archivo [LICENSE](LICENSE) para más detalles.