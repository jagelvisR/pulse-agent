#!/usr/bin/env bash
set -euo pipefail

REPO="jagelvisr/pulse-agent"
BIN_PATH="/usr/local/bin/pulse-agent"
CONFIG_DIR="/etc/pulse-agent"
SERVICE_PATH="/etc/systemd/system/pulse-agent.service"
SERVICE_USER="pulse-agent"

# Colores para salida limpia
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[+]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[x]${NC} $1" >&2; exit 1; }

# 1. Verificar privilegios de root
if [[ "${EUID}" -ne 0 ]]; then
  error "Este script debe ejecutarse como root (usa: sudo bash install.sh)"
fi

# 2. Detectar arquitectura del sistema
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64|amd64)
    ASSET_NAME="pulse-agent-linux-x86_64"
    ;;
  aarch64|arm64)
    ASSET_NAME="pulse-agent-linux-aarch64"
    ;;
  *)
    error "Arquitectura no soportada: ${ARCH}"
    ;;
esac

# 3. Verificar dependencias básicas de descarga
command -v curl >/dev/null 2>&1 || error "curl no está instalado. Instálalo con: apt-get install -y curl"

info "Detectada arquitectura ${ARCH} -> descargando ${ASSET_NAME}..."
TMP_BIN="$(mktemp)"
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET_NAME}"

curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_BIN}" || error "No se pudo descargar el binario desde ${DOWNLOAD_URL}"
chmod +x "${TMP_BIN}"

# 4. Crear usuario de sistema dedicado si no existe
if ! id -u "${SERVICE_USER}" >/dev/null 2>&1; then
  info "Creando usuario de sistema '${SERVICE_USER}'..."
  useradd --system --no-create-home --shell /usr/sbin/nologin "${SERVICE_USER}"
fi

# Agregar al grupo docker si existe en el servidor
if getent group docker >/dev/null 2>&1; then
  usermod -aG docker "${SERVICE_USER}"
fi

# 5. Instalar binario (deteniendo servicio temporalmente si estaba activo)
WAS_ACTIVE=false
if systemctl is-active --quiet pulse-agent; then
  WAS_ACTIVE=true
  info "Deteniendo servicio pulse-agent actual para actualizar binario..."
  systemctl stop pulse-agent
fi

mv "${TMP_BIN}" "${BIN_PATH}"
chown root:root "${BIN_PATH}"
chmod 755 "${BIN_PATH}"
info "Binario instalado en ${BIN_PATH} ($("${BIN_PATH}" --version 2>/dev/null || echo 'OK'))"

# 6. Crear directorio y archivos de configuración sin sobrescribir los existentes
mkdir -p "${CONFIG_DIR}"

if [[ ! -f "${CONFIG_DIR}/pulse.toml" ]]; then
  info "Descargando configuración inicial en ${CONFIG_DIR}/pulse.toml..."
  curl -fsSL "https://raw.githubusercontent.com/${REPO}/main/pulse.example.toml" -o "${CONFIG_DIR}/pulse.toml"
else
  warn "Se mantiene ${CONFIG_DIR}/pulse.toml existente sin sobrescribir."
fi

if [[ ! -f "${CONFIG_DIR}/pulse.env" ]]; then
  info "Creando plantilla de variables de entorno en ${CONFIG_DIR}/pulse.env..."
  cat > "${CONFIG_DIR}/pulse.env" <<EOF
PULSE_HOSTNAME=$(hostname)
# PULSE_DISCORD_WEBHOOK=https://discord.com/api/webhooks/TU_ID/TU_TOKEN
# PULSE_TELEGRAM_TOKEN=123456789:ABCdefGHIjklMNOpqrSTUvwxYZ
# PULSE_TELEGRAM_CHAT_ID=-1001234567890
# PULSE_HEARTBEAT_URL=https://hc-ping.com/tu-uuid-aqui
EOF
else
  warn "Se mantiene ${CONFIG_DIR}/pulse.env existente sin sobrescribir."
fi

chown -R root:"${SERVICE_USER}" "${CONFIG_DIR}"
chmod 750 "${CONFIG_DIR}"
chmod 640 "${CONFIG_DIR}/pulse.toml" "${CONFIG_DIR}/pulse.env"

# 7. Escribir unidad systemd endurecida
info "Configurando unidad systemd en ${SERVICE_PATH}..."
cat > "${SERVICE_PATH}" <<'EOF'
[Unit]
Description=pulse-agent - Lightweight System, Docker & HTTP Telemetry Agent
Documentation=https://github.com/jagelvisr/pulse-agent
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
User=pulse-agent
Group=pulse-agent
SupplementaryGroups=docker
EnvironmentFile=-/etc/pulse-agent/pulse.env
ExecStart=/usr/local/bin/pulse-agent --config /etc/pulse-agent/pulse.toml
Restart=always
RestartSec=5

# Endurecimiento de seguridad y límites de recursos
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
MemoryMax=48M
CPUQuota=25%

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable pulse-agent >/dev/null 2>&1

if [[ "${WAS_ACTIVE}" == "true" ]]; then
  systemctl start pulse-agent
  info "Servicio pulse-agent actualizado y reiniciado correctamente."
else
  systemctl start pulse-agent
  info "Servicio pulse-agent instalado y iniciado."
fi

echo ""
echo -e "${GREEN}Instalación completada.${NC}"
echo "  • Configuración TOML : ${CONFIG_DIR}/pulse.toml"
echo "  • Secretos / Entorno : ${CONFIG_DIR}/pulse.env"
echo "  • Probar alertas     : sudo -u pulse-agent bash -c 'set -a; source ${CONFIG_DIR}/pulse.env; set +a; pulse-agent --config ${CONFIG_DIR}/pulse.toml --test-alert'"
echo "  • Abrir panel TUI    : pulse-agent --config ${CONFIG_DIR}/pulse.toml --ui"
echo "  • Ver logs en vivo   : sudo journalctl -u pulse-agent -f"