use crate::metrics::{ContainerInfo, HttpCheckInfo, ProcessInfo, SharedMetrics, TelemetrySnapshot};
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, List, ListItem, ListState,
        Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState,
    },
    Frame, Terminal,
};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePane {
    Docker,
    Http,
    Processes,
    Logs,
}

impl ActivePane {
    fn next(self) -> Self {
        match self {
            ActivePane::Docker => ActivePane::Http,
            ActivePane::Http => ActivePane::Processes,
            ActivePane::Processes => ActivePane::Logs,
            ActivePane::Logs => ActivePane::Docker,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessSortBy {
    Cpu,
    Ram,
}

impl ProcessSortBy {
    fn toggle(self) -> Self {
        match self {
            ProcessSortBy::Cpu => ProcessSortBy::Ram,
            ProcessSortBy::Ram => ProcessSortBy::Cpu,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ProcessSortBy::Cpu => "CPU↓",
            ProcessSortBy::Ram => "RAM↓",
        }
    }
}

struct TuiState {
    active_pane: ActivePane,
    proc_sort: ProcessSortBy,
    search_mode: bool,
    filter_query: String,
    docker_state: TableState,
    http_state: TableState,
    proc_state: TableState,
    logs_state: ListState,
    h_scroll_docker: usize,
    h_scroll_http: usize,
    h_scroll_proc: usize,
    h_scroll_logs: usize,
    docker_area: Rect,
    http_area: Rect,
    proc_area: Rect,
    logs_area: Rect,
}

impl TuiState {
    fn new() -> Self {
        let mut docker_state = TableState::default();
        docker_state.select(Some(0));
        let mut http_state = TableState::default();
        http_state.select(Some(0));
        let mut proc_state = TableState::default();
        proc_state.select(Some(0));
        let mut logs_state = ListState::default();
        logs_state.select(Some(0));

        Self {
            active_pane: ActivePane::Docker,
            proc_sort: ProcessSortBy::Cpu,
            search_mode: false,
            filter_query: String::new(),
            docker_state,
            http_state,
            proc_state,
            logs_state,
            h_scroll_docker: 0,
            h_scroll_http: 0,
            h_scroll_proc: 0,
            h_scroll_logs: 0,
            docker_area: Rect::default(),
            http_area: Rect::default(),
            proc_area: Rect::default(),
            logs_area: Rect::default(),
        }
    }

    fn filtered_containers<'a>(&self, snap: &'a TelemetrySnapshot) -> Vec<&'a ContainerInfo> {
        let q = self.filter_query.trim().to_lowercase();
        snap.containers
            .iter()
            .filter(|c| {
                q.is_empty()
                    || c.name.to_lowercase().contains(&q)
                    || c.state.to_lowercase().contains(&q)
                    || c.status.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn filtered_http<'a>(&self, snap: &'a TelemetrySnapshot) -> Vec<&'a HttpCheckInfo> {
        let q = self.filter_query.trim().to_lowercase();
        snap.http_checks
            .iter()
            .filter(|h| {
                q.is_empty()
                    || h.name.to_lowercase().contains(&q)
                    || h.url.to_lowercase().contains(&q)
                    || h.status_label.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn filtered_and_sorted_procs<'a>(&self, snap: &'a TelemetrySnapshot) -> Vec<&'a ProcessInfo> {
        let q = self.filter_query.trim().to_lowercase();
        let mut list: Vec<&'a ProcessInfo> = snap
            .top_processes
            .iter()
            .filter(|p| {
                q.is_empty()
                    || p.name.to_lowercase().contains(&q)
                    || p.pid.to_string().contains(&q)
            })
            .collect();

        match self.proc_sort {
            ProcessSortBy::Cpu => list.sort_by(|a, b| {
                b.cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.ram_mb.cmp(&a.ram_mb))
            }),
            ProcessSortBy::Ram => list.sort_by(|a, b| {
                b.ram_mb.cmp(&a.ram_mb).then_with(|| {
                    b.cpu_percent
                        .partial_cmp(&a.cpu_percent)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }),
        }

        list
    }

    fn filtered_logs<'a>(&self, snap: &'a TelemetrySnapshot) -> Vec<&'a String> {
        let q = self.filter_query.trim().to_lowercase();
        snap.recent_logs
            .iter()
            .rev()
            .filter(|l| q.is_empty() || l.to_lowercase().contains(&q))
            .collect()
    }

    fn scroll_down(&mut self, d_len: usize, h_len: usize, p_len: usize, l_len: usize) {
        match self.active_pane {
            ActivePane::Docker if d_len > 0 => {
                let i = self.docker_state.selected().map_or(0, |i| (i + 1).min(d_len - 1));
                self.docker_state.select(Some(i));
            }
            ActivePane::Http if h_len > 0 => {
                let i = self.http_state.selected().map_or(0, |i| (i + 1).min(h_len - 1));
                self.http_state.select(Some(i));
            }
            ActivePane::Processes if p_len > 0 => {
                let i = self.proc_state.selected().map_or(0, |i| (i + 1).min(p_len - 1));
                self.proc_state.select(Some(i));
            }
            ActivePane::Logs if l_len > 0 => {
                let i = self.logs_state.selected().map_or(0, |i| (i + 1).min(l_len - 1));
                self.logs_state.select(Some(i));
            }
            _ => {}
        }
    }

    fn scroll_up(&mut self) {
        match self.active_pane {
            ActivePane::Docker => {
                let i = self.docker_state.selected().unwrap_or(0).saturating_sub(1);
                self.docker_state.select(Some(i));
            }
            ActivePane::Http => {
                let i = self.http_state.selected().unwrap_or(0).saturating_sub(1);
                self.http_state.select(Some(i));
            }
            ActivePane::Processes => {
                let i = self.proc_state.selected().unwrap_or(0).saturating_sub(1);
                self.proc_state.select(Some(i));
            }
            ActivePane::Logs => {
                let i = self.logs_state.selected().unwrap_or(0).saturating_sub(1);
                self.logs_state.select(Some(i));
            }
        }
    }

    fn scroll_right(&mut self) {
        match self.active_pane {
            ActivePane::Docker => self.h_scroll_docker = self.h_scroll_docker.saturating_add(4),
            ActivePane::Http => self.h_scroll_http = self.h_scroll_http.saturating_add(4),
            ActivePane::Processes => self.h_scroll_proc = self.h_scroll_proc.saturating_add(4),
            ActivePane::Logs => self.h_scroll_logs = self.h_scroll_logs.saturating_add(4),
        }
    }

    fn scroll_left(&mut self) {
        match self.active_pane {
            ActivePane::Docker => self.h_scroll_docker = self.h_scroll_docker.saturating_sub(4),
            ActivePane::Http => self.h_scroll_http = self.h_scroll_http.saturating_sub(4),
            ActivePane::Processes => self.h_scroll_proc = self.h_scroll_proc.saturating_sub(4),
            ActivePane::Logs => self.h_scroll_logs = self.h_scroll_logs.saturating_sub(4),
        }
    }

    fn select_pane_at(&mut self, col: u16, row: u16) {
        let pos = Position { x: col, y: row };
        if self.docker_area.contains(pos) {
            self.active_pane = ActivePane::Docker;
        } else if self.http_area.contains(pos) {
            self.active_pane = ActivePane::Http;
        } else if self.proc_area.contains(pos) {
            self.active_pane = ActivePane::Processes;
        } else if self.logs_area.contains(pos) {
            self.active_pane = ActivePane::Logs;
        }
    }

    fn clamp_selections(&mut self, snap: &TelemetrySnapshot) {
        let d_len = self.filtered_containers(snap).len();
        let h_len = self.filtered_http(snap).len();
        let p_len = self.filtered_and_sorted_procs(snap).len();
        let l_len = self.filtered_logs(snap).len();

        Self::clamp_table(&mut self.docker_state, d_len);
        Self::clamp_table(&mut self.http_state, h_len);
        Self::clamp_table(&mut self.proc_state, p_len);

        if l_len == 0 {
            self.logs_state.select(None);
        } else if let Some(i) = self.logs_state.selected() {
            if i >= l_len {
                self.logs_state.select(Some(l_len - 1));
            }
        } else {
            self.logs_state.select(Some(0));
        }
    }

    fn clamp_table(state: &mut TableState, len: usize) {
        if len == 0 {
            state.select(None);
        } else if let Some(i) = state.selected() {
            if i >= len {
                state.select(Some(len - 1));
            }
        } else {
            state.select(Some(0));
        }
    }
}

fn slice_horizontal(text: &str, offset: usize) -> String {
    text.chars().skip(offset).collect()
}

fn restore_terminal() {
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

pub async fn run_tui(hostname: String, metrics: SharedMetrics) -> Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    std::thread::spawn(move || loop {
        if let Ok(true) = event::poll(Duration::from_millis(100)) {
            if let Ok(ev) = event::read() {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        }
    });

    let res = tui_loop(&mut terminal, &hostname, &metrics, &mut rx).await;

    restore_terminal();
    let _ = terminal.show_cursor();

    res
}

async fn tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    hostname: &str,
    metrics: &SharedMetrics,
    rx: &mut mpsc::UnboundedReceiver<Event>,
) -> Result<()> {
    let mut ui_state = TuiState::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let snap = metrics.read().await.clone();
                ui_state.clamp_selections(&snap);
                let _ = terminal.draw(|f| draw_ui(f, hostname, &snap, &mut ui_state));
            }
            Some(ev) = rx.recv() => {
                let snap = metrics.read().await.clone();
                match ev {
                    Event::Key(key) => {
                        if key.kind != KeyEventKind::Press {
                            continue;
                        }

                        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                            return Ok(());
                        }

                        if ui_state.search_mode {
                            match key.code {
                                KeyCode::Enter => {
                                    ui_state.search_mode = false;
                                }
                                KeyCode::Esc => {
                                    ui_state.filter_query.clear();
                                    ui_state.search_mode = false;
                                }
                                KeyCode::Backspace => {
                                    ui_state.filter_query.pop();
                                }
                                KeyCode::Char(c) => {
                                    ui_state.filter_query.push(c);
                                }
                                _ => {}
                            }
                        } else {
                            let d_len = ui_state.filtered_containers(&snap).len();
                            let h_len = ui_state.filtered_http(&snap).len();
                            let p_len = ui_state.filtered_and_sorted_procs(&snap).len();
                            let l_len = ui_state.filtered_logs(&snap).len();

                            match key.code {
                                KeyCode::Char('q') => return Ok(()),
                                KeyCode::Char('/') => {
                                    ui_state.search_mode = true;
                                }
                                KeyCode::Char('s') => {
                                    ui_state.proc_sort = ui_state.proc_sort.toggle();
                                }
                                KeyCode::Tab => ui_state.active_pane = ui_state.active_pane.next(),
                                KeyCode::Down | KeyCode::Char('j') => {
                                    ui_state.scroll_down(d_len, h_len, p_len, l_len);
                                }
                                KeyCode::Up | KeyCode::Char('k') => ui_state.scroll_up(),
                                KeyCode::Right | KeyCode::Char('l') => ui_state.scroll_right(),
                                KeyCode::Left | KeyCode::Char('h') => ui_state.scroll_left(),
                                _ => {}
                            }
                        }
                    }
                    Event::Mouse(mouse) => {
                        let d_len = ui_state.filtered_containers(&snap).len();
                        let h_len = ui_state.filtered_http(&snap).len();
                        let p_len = ui_state.filtered_and_sorted_procs(&snap).len();
                        let l_len = ui_state.filtered_logs(&snap).len();

                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) => {
                                ui_state.select_pane_at(mouse.column, mouse.row);
                            }
                            MouseEventKind::ScrollDown => {
                                ui_state.select_pane_at(mouse.column, mouse.row);
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    ui_state.scroll_right();
                                } else {
                                    ui_state.scroll_down(d_len, h_len, p_len, l_len);
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                ui_state.select_pane_at(mouse.column, mouse.row);
                                if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                    ui_state.scroll_left();
                                } else {
                                    ui_state.scroll_up();
                                }
                            }
                            MouseEventKind::ScrollRight => {
                                ui_state.select_pane_at(mouse.column, mouse.row);
                                ui_state.scroll_right();
                            }
                            MouseEventKind::ScrollLeft => {
                                ui_state.select_pane_at(mouse.column, mouse.row);
                                ui_state.scroll_left();
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                ui_state.clamp_selections(&snap);
                let _ = terminal.draw(|f| draw_ui(f, hostname, &snap, &mut ui_state));
            }
        }
    }
}

fn draw_ui(f: &mut Frame, hostname: &str, snap: &TelemetrySnapshot, ui: &mut TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header / Barra de búsqueda
            Constraint::Length(9),  // Gráficas CPU y RAM
            Constraint::Length(3),  // Disco principal
            Constraint::Min(9),     // Docker + HTTP + Procesos
            Constraint::Length(6),  // Logs / Alertas recientes
        ])
        .split(f.area());

    // 1. Header o barra de búsqueda interactiva
    let header_line = if ui.search_mode {
        Line::from(vec![
            Span::styled(
                " 🔍 FILTRO: ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}█", ui.filter_query),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::raw("   ("),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(": Aplicar | "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(": Limpiar)"),
        ])
    } else {
        let mut spans = vec![
            Span::styled(
                " ⚡ PULSE-AGENT ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" | {} | Temp: {:.1}°C | ", hostname, snap.max_temp_c)),
            Span::styled("/", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(": Buscar | "),
            Span::styled("s", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(": Orden CPU/RAM | "),
            Span::styled("Tab/Click", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(": Panel | "),
            Span::styled("q", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(": Salir"),
        ];

        if !ui.filter_query.is_empty() {
            spans.push(Span::styled(
                format!("  [Filtro: \"{}\"]", ui.filter_query),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        }

        Line::from(spans)
    };

    let header_border = if ui.search_mode {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let header = Paragraph::new(header_line)
        .block(Block::default().borders(Borders::ALL).border_style(header_border));
    f.render_widget(header, chunks[0]);

    // 2. Gráficas CPU y RAM
    let charts_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_line_chart(
        f,
        charts_row[0],
        &format!(" CPU ({:.1}%) ", snap.cpu_percent),
        &snap.cpu_history,
        Color::Cyan,
    );
    render_line_chart(
        f,
        charts_row[1],
        &format!(" RAM ({:.1}%) ", snap.ram_percent),
        &snap.ram_history,
        Color::Magenta,
    );

    // 3. Disco principal
    let (disk_label, disk_pct) = snap
        .disks
        .first()
        .map(|d| (format!(" Disco {} ({:.1}%) ", d.mount, d.used_percent), d.used_percent))
        .unwrap_or((" Sin discos ".into(), 0.0));

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

    // 4. Paneles centrales
    let middle_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(36),
            Constraint::Percentage(34),
            Constraint::Percentage(30),
        ])
        .split(chunks[3]);

    ui.docker_area = middle_row[0];
    ui.http_area = middle_row[1];
    ui.proc_area = middle_row[2];
    ui.logs_area = chunks[4];

    render_docker_table(f, middle_row[0], snap, ui);
    render_http_table(f, middle_row[1], snap, ui);
    render_proc_table(f, middle_row[2], snap, ui);
    render_logs_list(f, chunks[4], snap, ui);
}

fn pane_border_style(is_active: bool) -> Style {
    if is_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_docker_table(f: &mut Frame, area: Rect, snap: &TelemetrySnapshot, ui: &mut TuiState) {
    let is_active = ui.active_pane == ActivePane::Docker;
    let offset = ui.h_scroll_docker;
    let items = ui.filtered_containers(snap);
    let count = items.len();

    let rows: Vec<Row> = items
        .into_iter()
        .map(|c| {
            let color = if c.state == "running" && !c.status.contains("unhealthy") {
                Color::Green
            } else {
                Color::Red
            };
            Row::new(vec![
                Span::raw(slice_horizontal(&c.name, offset)),
                Span::styled(c.state.clone(), Style::default().fg(color)),
                Span::raw(slice_horizontal(&c.status, offset)),
            ])
        })
        .collect();

    let title = format!(" Docker ({}/{}) [←{}→] ", count, snap.containers.len(), offset);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(22),
            Constraint::Percentage(38),
        ],
    )
        .header(
            Row::new(vec!["Contenedor", "Estado", "Detalle"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ")
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(pane_border_style(is_active)),
        );

    f.render_stateful_widget(table, area, &mut ui.docker_state);

    if count > 0 {
        let mut sb = ScrollbarState::new(count).position(ui.docker_state.selected().unwrap_or(0));
        f.render_stateful_widget(Scrollbar::new(ScrollbarOrientation::VerticalRight), area, &mut sb);
    }
}

fn render_http_table(f: &mut Frame, area: Rect, snap: &TelemetrySnapshot, ui: &mut TuiState) {
    let is_active = ui.active_pane == ActivePane::Http;
    let offset = ui.h_scroll_http;
    let items = ui.filtered_http(snap);
    let count = items.len();

    let rows: Vec<Row> = items
        .into_iter()
        .map(|h| {
            let color = if h.ok { Color::Green } else { Color::Red };
            let label_with_url = format!("{} ({})", h.name, h.url);
            Row::new(vec![
                Span::raw(slice_horizontal(&label_with_url, offset)),
                Span::styled(h.status_label.clone(), Style::default().fg(color)),
                Span::raw(format!("{}ms", h.latency_ms)),
            ])
        })
        .collect();

    let title = format!(" HTTP ({}/{}) [←{}→] ", count, snap.http_checks.len(), offset);
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(52),
            Constraint::Percentage(26),
            Constraint::Percentage(22),
        ],
    )
        .header(
            Row::new(vec!["Endpoint", "Estado", "ms"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ")
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(pane_border_style(is_active)),
        );

    f.render_stateful_widget(table, area, &mut ui.http_state);

    if count > 0 {
        let mut sb = ScrollbarState::new(count).position(ui.http_state.selected().unwrap_or(0));
        f.render_stateful_widget(Scrollbar::new(ScrollbarOrientation::VerticalRight), area, &mut sb);
    }
}

fn render_proc_table(f: &mut Frame, area: Rect, snap: &TelemetrySnapshot, ui: &mut TuiState) {
    let is_active = ui.active_pane == ActivePane::Processes;
    let offset = ui.h_scroll_proc;
    let items = ui.filtered_and_sorted_procs(snap);
    let count = items.len();

    let rows: Vec<Row> = items
        .into_iter()
        .map(|p| {
            let color = if p.cpu_percent > 50.0 || p.ram_mb > 1500 {
                Color::Yellow
            } else {
                Color::White
            };
            Row::new(vec![
                Span::styled(slice_horizontal(&p.name, offset), Style::default().fg(color)),
                Span::raw(format!("{:.1}%", p.cpu_percent)),
                Span::raw(format!("{}M", p.ram_mb)),
            ])
        })
        .collect();

    let title = format!(
        " Top SO ({}/{}) [{}] [←{}→] ",
        count,
        snap.top_processes.len(),
        ui.proc_sort.label(),
        offset
    );
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
        .header(
            Row::new(vec!["App/Proc", "CPU", "RAM"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ")
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(pane_border_style(is_active)),
        );

    f.render_stateful_widget(table, area, &mut ui.proc_state);

    if count > 0 {
        let mut sb = ScrollbarState::new(count).position(ui.proc_state.selected().unwrap_or(0));
        f.render_stateful_widget(Scrollbar::new(ScrollbarOrientation::VerticalRight), area, &mut sb);
    }
}

fn render_logs_list(f: &mut Frame, area: Rect, snap: &TelemetrySnapshot, ui: &mut TuiState) {
    let is_active = ui.active_pane == ActivePane::Logs;
    let offset = ui.h_scroll_logs;
    let items = ui.filtered_logs(snap);
    let count = items.len();

    let log_items: Vec<ListItem> = items
        .into_iter()
        .map(|l| ListItem::new(Line::from(slice_horizontal(l, offset))))
        .collect();

    let title = format!(
        " Actividad y Alertas ({}/{}) [←{}→] ",
        count,
        snap.recent_logs.len(),
        offset
    );
    let logs_list = List::new(log_items)
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ")
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(pane_border_style(is_active)),
        );

    f.render_stateful_widget(logs_list, area, &mut ui.logs_state);

    if count > 0 {
        let mut sb = ScrollbarState::new(count).position(ui.logs_state.selected().unwrap_or(0));
        f.render_stateful_widget(Scrollbar::new(ScrollbarOrientation::VerticalRight), area, &mut sb);
    }
}

fn render_line_chart(
    f: &mut Frame,
    area: Rect,
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