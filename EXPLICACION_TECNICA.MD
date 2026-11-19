# Documentación Técnica y Arquitectura: `pulse-agent`

## 1. ¿Qué es `pulse-agent`?

**`pulse-agent`** es un agente nativo de telemetría, monitoreo en consola y alertas instantáneas escrito en **Rust**. Está diseñado para operar desde un único binario estático de menos de 10 MB de RAM en tres flujos operativos:

1. **Modo Demonio / Segundo Plano (`pulse-agent`):** Corre de forma silenciosa supervisando el servidor, procesos del sistema operativo, contenedores Docker (incluyendo bucles de reinicio), endpoints HTTP y fechas de vencimiento de certificados SSL/TLS, enviando alertas a Discord y Telegram únicamente cuando ocurre una falla o recuperación, además de emitir pings periódicos de *Heartbeat*.
2. **Modo Interactivo en Consola (`pulse-agent --ui`):** Despliega una interfaz gráfica en terminal (TUI) construida con `ratatui` que dibuja curvas históricas de CPU y RAM, barras de uso de disco, estado en vivo de contenedores Docker con contador de reinicios, latencia de APIs con días restantes de SSL, top de procesos pesados del sistema operativo (ordenable por CPU o RAM), registro de eventos y búsqueda/filtrado en tiempo real.
3. **Verificación Rápida (`pulse-agent --test-alert`):** Valida la configuración TOML y las variables de entorno enviando una notificación de prueba inmediata a todos los canales habilitados y finalizando con código de salida `0` o error explícito.

---

## 2. Arquitectura Interna

El proyecto sigue una arquitectura modular desacoplada donde la carga de configuración híbrida, la recolección de telemetría, la evaluación de estados, el ping de supervivencia (*Heartbeat*), la captura de entrada de consola y la visualización operan de forma independiente:

```text
           ┌─────────────────────┐      ┌──────────────────────────┐
           │     pulse.toml      │  +   │  Variables de Entorno    │
           └──────────┬──────────┘      │ (PULSE_DISCORD_WEBHOOK,  │
                      │                 │  PULSE_TELEGRAM_*, etc.) │
                      ▼                 └────────────┬─────────────┘
           ┌─────────────────────┐                   │
           │    Config::load     │◄──────────────────┘
           └──────────┬──────────┘
                      │
      ┌───────────────┼───────────────┬────────────────────┐
      ▼               ▼               ▼                    ▼
┌────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐
│  System    │ │   Docker    │ │    HTTP     │ │   Heartbeat Loop    │
│ Collector  │ │  Collector  │ │  Collector  │ │ (Dead Man's Switch) │
│(CPU/RAM/T/ │ │(Socket/Pipe │ │(HTTP Status │ └──────────┬──────────┘
│Top Procs)  │ │+RestartCnt) │ │+SSL X.509)  │            │
└─────┬──────┘ └──────┬──────┘ └──────┬──────┘            ▼
      │               │               │          [Ping HTTP GET externo]
      └───────────────┼───────────────┘
                      ▼
        ┌───────────────────────────┐
        │       StateManager        │
        │ (Cooldown & Recuperación) │
        └──────┬─────────────┬──────┘
               │             │
               ▼             ▼
┌──────────────────┐   ┌───────────────────────────┐
│   Notifiers      │   │     TelemetrySnapshot     │
│ (Discord/Telegr.)│   │ (Buffer circular en RAM)  │
└──────────────────┘   └─────────────┬─────────────┘
                                     │
                                     ▼
┌───────────────────────────┐ ┌───────────────────────────┐
│ Hilo Dedicado de Eventos  │ │     Terminal UI (--ui)    │
│ (Teclado/Mouse -> mpsc)   ├─► (Filtro '/', Orden 's',   │
└───────────────────────────┘ │  Scroll Vertical/Horiz.)  │
                              └───────────────────────────┘
```

### Componentes Principales

- **`src/main.rs` (Orquestador, CLI y Heartbeat):**
    - Parsea los flags `--config`, `--ui` y `--test-alert` con `clap`.
    - Si `--test-alert` está activo, ejecuta `run_test_alert(&cfg)` y termina sin levantar bucles de monitoreo.
    - Si `[heartbeat]` está habilitado (en TOML o vía `PULSE_HEARTBEAT_URL`), lanza la tarea asíncrona `run_heartbeat_loop` en Tokio para enviar peticiones `GET` periódicas cada `interval_secs`.
    - En modo `--ui` ejecuta los *collectors* en segundo plano cada 2 segundos mientras el hilo principal renderiza la TUI; en modo demonio ejecuta el muestreo cada `check_interval_secs` y atiende señales `SIGINT`/`SIGTERM`.
- **`src/config.rs` (Configuración Tipada y Overrides de Entorno):**
    - Deserializa `pulse.toml` mediante `serde` y `toml` aplicando valores por defecto seguros (`process_cpu_threshold_percent = 80.0`, `process_ram_threshold_mb = 2048`, `restart_delta_threshold = 1`, `ssl_expiry_warning_days = 14`, `interval_secs = 300`).
    - Ejecuta `apply_env_overrides()` tras leer el archivo para sobrescribir y activar dinámicamente credenciales desde `PULSE_HOSTNAME`, `PULSE_DISCORD_WEBHOOK`, `PULSE_TELEGRAM_TOKEN`, `PULSE_TELEGRAM_CHAT_ID` y `PULSE_HEARTBEAT_URL`.
- **`src/collectors/` (Recolectores de Telemetría):**
    - `system.rs`: Consulta uso global de CPU, memoria RAM, espacio disponible por partición y sensores de temperatura mediante `sysinfo`. Inspecciona todos los procesos activos (`ProcessesToUpdate::All`), normaliza su porcentaje de CPU entre el total de núcleos lógicos, conserva el top 10 para la TUI y evalúa alertas sobre los 15 procesos más pesados.
    - `docker.rs`: Se conecta directamente al socket Unix (`/var/run/docker.sock`) o *named pipe* de Windows mediante `bollard`. Soporta lista blanca vacía (`[]`) o coincidencia por prefijo (`starts_with`) para Docker Compose/Swarm. Además, ejecuta `inspect_container` para leer `RestartCount`, almacena un mapa histórico `last_restart_counts` y dispara alertas de *CrashLoop* cuando el incremento entre ciclos alcanza `restart_delta_threshold`.
    - `http.rs`: Ejecuta peticiones HTTP asíncronas con `reqwest` midiendo código de estado y latencia en milisegundos. Para endpoints `https://`, abre una sesión TLS nativa con `tokio-rustls` + `webpki-roots`, extrae el certificado hoja en formato DER, lo decodifica con `x509-parser` y calcula los días restantes hasta `not_after` para alertar antes de su caducidad (`Warning` ≤ `ssl_expiry_warning_days`, `Critical` ≤ 3 días).
- **`src/state.rs` (Máquina de Estados y Cooldown):** Mantiene un mapa en memoria (`HashMap<String, Instant>`) de incidentes activos por clave única (`sys:cpu`, `sys:proc:...`, `docker:container:...`, `docker:restarts:...`, `http:...`, `ssl:...`). Evita el spam de mensajes repetidos mientras dura una falla y emite automáticamente un evento de severidad `Resolved` cuando la métrica vuelve a la normalidad.
- **`src/metrics.rs` (Historial Circular en Memoria):** Estructura protegida por `Arc<RwLock<TelemetrySnapshot>>` que almacena los últimos 60 puntos de CPU y RAM (`VecDeque<f64>`), estado actual de discos, top 10 de procesos del SO, contenedores (con indicador `(↻ N)` de reinicios), checks HTTP (nombre, URL, estado, latencia y días SSL) y los últimos 100 eventos en `recent_logs` sin escribir nada en disco.
- **`src/tui.rs` (Interfaz Gráfica en Consola):**
    - Renderiza gráficos de líneas con marcadores Braille (`Chart`), medidor porcentual de disco (`Gauge`), tres tablas interactivas (**Docker**, **HTTP** y **Top SO**) con `TableState` y lista de actividad (`List`) con `ListState` y barras `Scrollbar`.
    - Implementa búsqueda interactiva en vivo (`/`) que filtra simultáneamente contenedores, endpoints HTTP, procesos del SO y el registro de eventos, junto con alternancia dinámica de ordenamiento (`s`) en **Top SO** entre `CPU↓` y `RAM↓`.
    - Aísla la lectura bloqueante de eventos de `crossterm` en un hilo dedicado (`std::thread::spawn`) conectado vía canal asíncrono `tokio::sync::mpsc`, incluye *panic hook* de restauración de terminal y soporta teclado (`Tab`, `↑/↓/j/k`, `←/→/h/l`) y mouse (clic para foco, rueda para scroll vertical y `Shift + Rueda` para scroll horizontal).
- **`src/notifiers/` (Canales de Salida):** Define el *trait* asíncrono `Notifier` implementado por `DiscordNotifier` (embeds JSON con código de color) y `TelegramNotifier` (mensajes formateados en HTML).

---

## 3. Flujo de Ejecución Paso a Paso

1. **Carga inicial y overrides:** `pulse-agent` lee `pulse.toml`, aplica las variables de entorno `PULSE_*` e inicializa los notificadores habilitados. Si se invocó `--test-alert`, envía la alerta de prueba y finaliza.
2. **Supervisión de supervivencia (*Heartbeat*):** Si `[heartbeat]` está activo, inicia en paralelo el bucle periódico que notifica a la URL externa que el agente y la red del nodo siguen operativos.
3. **Ciclo de muestreo:** Cada `N` segundos (30s por defecto en demonio, 2s en `--ui`), los tres *collectors* obtienen métricas frescas del host, procesos del SO, contenedores Docker (`State` + `RestartCount`) y endpoints HTTP/SSL.
4. **Actualización de instantánea:** Se guarda el nuevo punto en `TelemetrySnapshot` (manteniendo una ventana deslizante de 60 muestras para las gráficas y 100 líneas para el historial de eventos).
5. **Evaluación de umbrales:** `StateManager` compara métricas globales, procesos individuales, contenedores, reinicios, respuestas HTTP y días de validez SSL contra los límites configurados:
    - Si supera el umbral y no está en *cooldown*, genera un `AlertEvent` (`Warning` o `Critical`).
    - Si estaba en alerta y volvió a niveles normales, genera un `AlertEvent` (`Resolved`).
6. **Despacho y renderizado:** Cada `AlertEvent` generado se registra en `recent_logs` (filtrable en `--ui`) y se envía de forma asíncrona a Discord y/o Telegram.

---

## 4. Ventajas Operativas frente a Stacks Tradicionales

| Característica | `pulse-agent` | Prometheus + Grafana + Alertmanager |
| :--- | :--- | :--- |
| **Consumo de RAM** | ~6 – 10 MB | ~500 MB – 1.5 GB |
| **Consumo de Disco** | 0 MB (sin base de datos) | Varios GB (series temporales) |
| **Dependencias** | Ninguna (binario estático con `rustls`) | Múltiples contenedores / servicios |
| **Tiempo de despliegue** | < 1 minuto (TOML o variables `PULSE_*`) | Configuración compleja de exporters y reglas |
| **Visualización en SSH** | Sí (`--ui` nativo con búsqueda `/` y orden `s`) | No (requiere navegador web) |
| **Detalle de Procesos SO** | Sí (Top 10 en vivo + alertas por proceso) | Requiere `process-exporter` adicional |
| **SSL y CrashLoop Docker** | Integrados de serie (`x509-parser` + `bollard`) | Requieren `blackbox_exporter` y reglas PromQL |lertas por proceso) | Requiere `process-exporter` adicional |