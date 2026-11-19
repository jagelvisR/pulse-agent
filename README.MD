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
- **Nativo para Docker:** Se conecta directamente a `/var/run/docker.sock` (Linux) o al *named pipe* de Docker Desktop (Windows) para detectar contenedores detenidos, en estado `unhealthy` o con bucles de reinicio (`RestartCount`), soportando filtrado por prefijo de nombre para Docker Compose y Swarm.
- **Healthchecks HTTP y Vencimiento SSL/TLS:** Sondea periódicamente endpoints verificando códigos de estado HTTP esperados, tiempos límite (*timeouts*), latencia de respuesta y días restantes antes de que caduque el certificado X.509 en URLs `https://`.
- **Heartbeat (*Dead Man's Switch*):** Envía pings periódicos a servicios externos (Healthchecks.io, Uptime Kuma) para avisar si el servidor completo pierde conectividad.
- **Alertas Inteligentes, Cooldown y Secretos por Entorno:** Gestor de estado integrado que evita el spam de notificaciones repetidas, envía avisos automáticos de **Recuperado**, permite probar canales con `--test-alert` y soporta variables de entorno para credenciales.

---

## 🚀 Inicio Rápido

### Opción 1: Docker Compose (Recomendado para Contenedores)

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
    read_only: true
    security_opt:
      - no-new-privileges:true
    environment:
      - PULSE_HOSTNAME=prod-vps-01
      - PULSE_DISCORD_WEBHOOK=[https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN](https://discord.com/api/webhooks/YOUR_ID/YOUR_TOKEN)
      # - PULSE_TELEGRAM_TOKEN=123456789:ABCdefGHIjklMNOpqrSTUvwxYZ
      # - PULSE_TELEGRAM_CHAT_ID=-1001234567890
      # - PULSE_HEARTBEAT_URL=[https://hc-ping.com/tu-uuid-aqui](https://hc-ping.com/tu-uuid-aqui)
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
      - ./pulse.toml:/config/pulse.toml:ro
    deploy:
      resources:
        limits:
          cpus: "0.25"
          memory: 32M
        reservations:
          cpus: "0.05"
          memory: 10M
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

### Opción 3: Servicio Nativo `systemd` en Linux (Instalación/Actualización Automática)

Instala o actualiza `pulse-agent` como servicio del sistema en Ubuntu/Debian (`x86_64` o `ARM64`) en un solo comando. El instalador crea el usuario sin privilegios `pulse-agent`, configura `/etc/pulse-agent/` sin sobrescribir tus archivos existentes y registra la unidad `systemd`:

```bash
curl -fsSL [https://raw.githubusercontent.com/jagelvisr/pulse-agent/main/install.sh](https://raw.githubusercontent.com/jagelvisr/pulse-agent/main/install.sh) | sudo bash
```

Luego configura tus webhooks en `/etc/pulse-agent/pulse.env` y tus endpoints en `/etc/pulse-agent/pulse.toml`:

```bash
sudo nano /etc/pulse-agent/pulse.env
sudo nano /etc/pulse-agent/pulse.toml
sudo systemctl restart pulse-agent
```

Comandos habituales de administración con `systemd`:

```bash
# Ver estado y consumo exacto de memoria:
sudo systemctl status pulse-agent

# Ver registros en tiempo real:
sudo journalctl -u pulse-agent -f

# Abrir la consola interactiva TUI sin detener el demonio:
pulse-agent --config /etc/pulse-agent/pulse.toml --ui
```

### Opción 4: Binario Independiente Manual

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

## 💻 Referencia de Comandos CLI y Atajos TUI

### Flags de Línea de Comandos

| Comando / Flag | Descripción |
| :--- | :--- |
| `pulse-agent` | Ejecuta el agente en modo demonio en segundo plano usando `pulse.toml`. |
| `pulse-agent --config <RUTA>` | Especifica una ruta personalizada para el archivo de configuración TOML. |
| `pulse-agent --ui` | Abre la interfaz gráfica interactiva en consola (TUI) con muestreo cada 2s. |
| `pulse-agent --test-alert` | Envía una alerta de prueba a los canales activos (Discord/Telegram) y sale. |
| `pulse-agent --help` | Muestra la ayuda completa de argumentos y versión del binario. |

### Atajos Interactivos en Modo `--ui`

| Tecla / Acción | Función |
| :--- | :--- |
| `/` | Activa el modo de búsqueda en vivo sobre todos los paneles (`Docker`, `HTTP`, `Top SO`, `Logs`). |
| `Enter` | Fija el filtro escrito y devuelve el control a la navegación normal. |
| `Esc` | Borra el texto del filtro activo y cierra la barra de búsqueda. |
| `s` | Alterna el orden de la tabla **Top SO** entre mayor consumo de **CPU** (`CPU↓`) y **RAM** (`RAM↓`). |
| `Tab` o **Clic izquierdo** | Cambia el panel activo (resaltado con borde cian). |
| `↑` / `↓` o `j` / `k` o **Rueda** | Desplaza verticalmente las filas del panel activo. |
| `←` / `→` o `h` / `l` o `Shift + Rueda` | Desplaza horizontalmente columnas y URLs largas. |
| `q` o `Ctrl+C` | Restaura la terminal y cierra la aplicación de forma segura. |

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

### Parámetros de `[[http_checks]]` y `[heartbeat]`

| Sección / Campo | Tipo | Por defecto | Descripción |
| :--- | :--- | :--- | :--- |
| `http_checks.name` | `String` | *(Requerido)* | Nombre identificador del endpoint en alertas y en la tabla TUI. |
| `http_checks.url` | `String` | *(Requerido)* | URL completa a monitorear (`http://` o `https://`). |
| `http_checks.expected_status` | `u16` | `200` | Código HTTP esperado para considerar el servicio saludable. |
| `http_checks.timeout_secs` | `u64` | `5` | Tiempo máximo de espera antes de marcar caída o timeout. |
| `http_checks.ssl_expiry_warning_days` | `i64` | `14` | Días mínimos de vigencia del certificado TLS/X.509 antes de alertar (`0` desactiva el chequeo SSL). |
| `heartbeat.enabled` | `bool` | `false` | Activa el envío periódico de pings de supervivencia (*Dead Man's Switch*). |
| `heartbeat.url` | `String` | — | URL receptora del ping HTTP `GET` (ej. Healthchecks.io o Uptime Kuma). |
| `heartbeat.interval_secs` | `u64` | `300` | Frecuencia en segundos entre cada ping de Heartbeat (mínimo `10`). |

### Ejemplo Completo (`pulse.toml`)

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