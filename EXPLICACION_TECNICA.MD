# Documentación Técnica y Arquitectura: `pulse-agent`

## 1. ¿Qué es `pulse-agent`?

**`pulse-agent`** es un agente nativo de telemetría, monitoreo en consola y alertas instantáneas escrito en **Rust**. Está diseñado para operar en dos modos desde un único binario estático de menos de 10 MB de RAM:

1. **Modo Demonio / Segundo Plano (`pulse-agent`):** Corre de forma silenciosa supervisando el servidor, contenedores Docker y endpoints HTTP, enviando alertas a Discord y Telegram únicamente cuando ocurre una falla o recuperación.
2. **Modo Interactivo en Consola (`pulse-agent --ui`):** Despliega una interfaz gráfica en terminal (TUI) construida con `ratatui` que dibuja curvas históricas de CPU y RAM, barras de uso de disco, estado en vivo de contenedores Docker, latencia de APIs y registro de eventos.

---

## 2. Arquitectura Interna

El proyecto sigue una arquitectura modular desacoplada donde la recolección de datos, la evaluación de estados, la visualización y el envío de alertas operan de forma independiente:

```text
                    ┌───────────────────────┐
                    │      pulse.toml       │
                    └───────────┬───────────┘
                                │
             ┌──────────────────┼──────────────────┐
             ▼                  ▼                  ▼
   ┌──────────────────┐ ┌───────────────┐ ┌────────────────┐
   │ SystemCollector  │ │DockerCollector│ │ HttpCollector  │
   │ (CPU/RAM/Disco/T)│ │  (Socket/Pipe)│ │ (Status/ms)    │
   └─────────┬────────┘ └───────┬───────┘ └───────┬────────┘
             │                  │                 │
             └──────────────────┼─────────────────┘
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
                                 ┌───────────────────────────┐
                                 │     Terminal UI (--ui)    │
                                 │    (ratatui + crossterm)  │
                                 └───────────────────────────┘
```

### Componentes Principales

* **`src/main.rs` (Orquestador y CLI):** Parsea los argumentos de línea de comandos con `clap` (`--config`, `--ui`). En modo normal ejecuta el bucle de recolección cada `check_interval_secs` y escucha señales del sistema operativo (`SIGINT`/`SIGTERM`). En modo `--ui` lanza los *collectors* en una tarea asíncrona de Tokio cada 2 segundos mientras el hilo principal renderiza la interfaz en pantalla.
* **`src/config.rs` (Configuración Tipada):** Deserializa `pulse.toml` mediante `serde` y `toml`, aplicando valores por defecto seguros si algún campo opcional se omite.
* **`src/collectors/` (Recolectores de Telemetría):**
    * `system.rs`: Consulta uso global de CPU, memoria RAM, espacio disponible por partición y sensores de temperatura mediante `sysinfo`.
    * `docker.rs`: Se conecta directamente al socket Unix (`/var/run/docker.sock`) o *named pipe* de Windows mediante `bollard` para inspeccionar contenedores activos, detenidos o marcados como `unhealthy`.
    * `http.rs`: Ejecuta peticiones HTTP asíncronas con `reqwest` + `rustls`, midiendo código de respuesta y latencia en milisegundos.
* **`src/state.rs` (Máquina de Estados y Cooldown):** Mantiene un mapa en memoria (`HashMap<String, Instant>`) de incidentes activos. Evita el spam de mensajes repetidos mientras dura una falla y emite automáticamente un evento de severidad `Resolved` cuando la métrica vuelve a la normalidad.
* **`src/metrics.rs` (Historial Circular en Memoria):** Estructura protegida por `Arc<RwLock<TelemetrySnapshot>>` que almacena los últimos 60 puntos de CPU y RAM (`VecDeque<f64>`), estado actual de discos, contenedores y checks HTTP sin escribir nada en disco.
* **`src/tui.rs` (Interfaz Gráfica en Consola):** Renderiza gráficos de líneas con marcadores Braille (`Chart`), medidores porcentuales (`Gauge`), tablas (`Table`) y lista de actividad (`List`) usando `ratatui` y `crossterm`.
* **`src/notifiers/` (Canales de Salida):** Define el *trait* asíncrono `Notifier` implementado por `DiscordNotifier` (embeds JSON con código de color) y `TelegramNotifier` (mensajes formateados en HTML).

---

## 3. Flujo de Ejecución Paso a Paso

1. **Carga inicial:** Al arrancar, `pulse-agent` lee `pulse.toml`, valida la sintaxis e inicializa los notificadores habilitados.
2. **Ciclo de muestreo:** Cada `N` segundos (30s por defecto en demonio, 2s en `--ui`), los tres *collectors* obtienen métricas frescas del host, Docker y endpoints HTTP.
3. **Actualización de instantánea:** Se guarda el nuevo punto en `TelemetrySnapshot` (descartando el punto más antiguo si ya hay 60 muestras en memoria).
4. **Evaluación de umbrales:** `StateManager` compara los valores contra los límites definidos en `pulse.toml`:
    * Si supera el umbral y no está en *cooldown*, genera un `AlertEvent` (`Warning` o `Critical`).
    * Si estaba en alerta y volvió a niveles normales, genera un `AlertEvent` (`Resolved`).
5. **Despacho de notificaciones:** Cada `AlertEvent` generado se registra en `recent_logs` y se envía de forma asíncrona a Discord y/o Telegram.

---

## 4. Ventajas Operativas frente a Stacks Tradicionales

| Característica | `pulse-agent` | Prometheus + Grafana + Alertmanager |
| :--- | :--- | :--- |
| **Consumo de RAM** | ~6 – 10 MB | ~500 MB – 1.5 GB |
| **Consumo de Disco** | 0 MB (sin base de datos) | Varios GB (series temporales) |
| **Dependencias** | Ninguna (binario estático) | Múltiples contenedores / servicios |
| **Tiempo de despliegue** | < 1 minuto | Configuración compleja de exporters y reglas |
| **Visualización en SSH** | Sí (`--ui` nativo en terminal) | No (requiere navegador web) |