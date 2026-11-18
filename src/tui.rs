use crate::metrics::{SharedMetrics, TelemetrySnapshot};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, List, ListItem, Paragraph, Row, Table},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

pub async fn run_tui(hostname: String, metrics: SharedMetrics) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = tui_loop(&mut terminal, &hostname, &metrics).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

async fn tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    hostname: &str,
    metrics: &SharedMetrics,
) -> Result<()> {
    loop {
        let snap = metrics.read().await.clone();
        terminal.draw(|f| draw_ui(f, hostname, &snap))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn draw_ui(f: &mut Frame, hostname: &str, snap: &TelemetrySnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(11), // Gráficas CPU y RAM
            Constraint::Length(5),  // Discos y Temp
            Constraint::Min(8),     // Docker + HTTP Checks
            Constraint::Length(6),  // Logs / Alertas recientes
        ])
        .split(f.area());

    // 1. Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " ⚡ PULSE-AGENT ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            " | Host: {} | Temp Máx: {:.1}°C | Presiona ",
            hostname, snap.max_temp_c
        )),
        Span::styled(
            "'q'",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" para salir"),
    ]))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // 2. Gráficos de líneas (CPU y RAM)
    let charts_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_line_chart(
        f,
        charts_row[0],
        &format!("CPU ({:.1}%)", snap.cpu_percent),
        &snap.cpu_history,
        Color::Cyan,
    );
    render_line_chart(
        f,
        charts_row[1],
        &format!("RAM ({:.1}%)", snap.ram_percent),
        &snap.ram_history,
        Color::Magenta,
    );

    // 3. Discos (Gauge principal del primer disco)
    let (disk_label, disk_pct) = snap
        .disks
        .first()
        .map(|d| (format!("Disco {} ({:.1}%)", d.mount, d.used_percent), d.used_percent))
        .unwrap_or(("Sin discos".into(), 0.0));

    let gauge_color = if disk_pct > 85.0 {
        Color::Red
    } else if disk_pct > 70.0 {
        Color::Yellow
    } else {
        Color::Green
    };
    let disk_gauge = Gauge::default()
        .block(Block::default().title(disk_label).borders(Borders::ALL))
        .gauge_style(Style::default().fg(gauge_color))
        .percent(disk_pct.clamp(0.0, 100.0) as u16);
    f.render_widget(disk_gauge, chunks[2]);

    // 4. Tabla Docker y HTTP Checks lado a lado
    let middle_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[3]);

    let docker_rows: Vec<Row> = snap
        .containers
        .iter()
        .map(|c| {
            let color = if c.state == "running" && !c.status.contains("unhealthy") {
                Color::Green
            } else {
                Color::Red
            };
            Row::new(vec![
                Span::raw(c.name.clone()),
                Span::styled(c.state.clone(), Style::default().fg(color)),
                Span::raw(c.status.clone()),
            ])
        })
        .collect();

    let docker_table = Table::new(
        docker_rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(20),
            Constraint::Percentage(40),
        ],
    )
        .header(
            Row::new(vec!["Contenedor", "Estado", "Detalle"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().title(" Contenedores Docker ").borders(Borders::ALL));
    f.render_widget(docker_table, middle_row[0]);

    let http_rows: Vec<Row> = snap
        .http_checks
        .iter()
        .map(|h| {
            let color = if h.ok { Color::Green } else { Color::Red };
            Row::new(vec![
                Span::raw(format!("{} ({})", h.name, h.url)),
                Span::styled(h.status_label.clone(), Style::default().fg(color)),
                Span::raw(format!("{} ms", h.latency_ms)),
            ])
        })
        .collect();

    let http_table = Table::new(
        http_rows,
        [
            Constraint::Percentage(45),
            Constraint::Percentage(30),
            Constraint::Percentage(25),
        ],
    )
        .header(
            Row::new(vec!["Endpoint", "Estado", "Latencia"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().title(" HTTP Checks ").borders(Borders::ALL));
    f.render_widget(http_table, middle_row[1]);

    // 5. Log de eventos recientes
    let log_items: Vec<ListItem> = snap
        .recent_logs
        .iter()
        .rev()
        .map(|l| ListItem::new(Line::from(l.as_str())))
        .collect();

    let logs_list =
        List::new(log_items).block(Block::default().title(" Actividad y Alertas ").borders(Borders::ALL));
    f.render_widget(logs_list, chunks[4]);
}

fn render_line_chart(
    f: &mut Frame,
    area: ratatui::layout::Rect,
    title: &str,
    history: &std::collections::VecDeque<f64>,
    color: Color,
) {
    let points: Vec<(f64, f64)> = history
        .iter()
        .enumerate()
        .map(|(i, &val)| (i as f64, val))
        .collect();

    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(color))
        .data(&points);

    let chart = Chart::new(vec![dataset])
        .block(Block::default().title(title).borders(Borders::ALL))
        .x_axis(Axis::default().bounds([0.0, 60.0]))
        .y_axis(
            Axis::default()
                .bounds([0.0, 100.0])
                .labels(vec![Line::from("0%"), Line::from("50%"), Line::from("100%")]),
        );

    f.render_widget(chart, area);
}