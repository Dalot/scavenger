use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph, Row, Table},
};
use serde_json::Value;

use crate::daemon;

struct AppState {
    scavenger_dir: std::path::PathBuf,
    interval: Duration,
    last_metrics: Option<Value>,
    recent_log_lines: Vec<LogEntry>,
    last_refresh: Instant,
    error: Option<String>,
}

struct LogEntry {
    time: String,
    level: String,
    message: String,
    fields: String,
}

pub fn run(scavenger_dir: &Path, interval_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppState {
        scavenger_dir: scavenger_dir.to_path_buf(),
        interval: Duration::from_secs(interval_secs),
        last_metrics: None,
        recent_log_lines: Vec::new(),
        last_refresh: Instant::now() - Duration::from_secs(999),
        error: None,
    };

    loop {
        if app.last_refresh.elapsed() >= app.interval {
            refresh_data(&mut app);
            app.last_refresh = Instant::now();
        }

        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('r') => {
                        refresh_data(&mut app);
                        app.last_refresh = Instant::now();
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn refresh_data(app: &mut AppState) {
    let socket_path = app.scavenger_dir.join("daemon.sock");
    if !socket_path.exists() {
        app.error = Some("Daemon not running (no socket)".into());
        app.last_metrics = None;
        return;
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            app.error = Some(format!("Runtime error: {e}"));
            return;
        }
    };

    let request = serde_json::json!({"method": "metrics"});
    match rt.block_on(daemon::socket::send_request(&socket_path, &request)) {
        Ok(val) => {
            app.last_metrics = Some(val);
            app.error = None;
        }
        Err(e) => {
            app.error = Some(format!("Daemon connection failed: {e}"));
            app.last_metrics = None;
        }
    }

    refresh_log_entries(app);
}

fn refresh_log_entries(app: &mut AppState) {
    let mut log_files: Vec<_> = std::fs::read_dir(&app.scavenger_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("daemon.log"))
        .collect();

    if log_files.is_empty() {
        return;
    }

    log_files.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));

    let content = std::fs::read_to_string(log_files[0].path()).unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(15);

    app.recent_log_lines.clear();
    for line in &lines[start..] {
        if let Ok(parsed) = serde_json::from_str::<Value>(line) {
            let level = parsed
                .get("level")
                .and_then(|v| v.as_str())
                .unwrap_or("INFO")
                .to_string();
            let ts = parsed
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ts_short = if ts.len() > 19 {
                ts[11..19].to_string()
            } else {
                ts.to_string()
            };

            let fields = parsed.get("fields").and_then(|v| v.as_object());
            let message = fields
                .and_then(|f| f.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut extra = String::new();
            if let Some(f) = fields {
                for (k, v) in f {
                    if k == "message" {
                        continue;
                    }
                    if !extra.is_empty() {
                        extra.push_str(", ");
                    }
                    extra.push_str(&format!("{k}={v}"));
                }
            }

            app.recent_log_lines.push(LogEntry {
                time: ts_short,
                level,
                message,
                fields: extra,
            });
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &AppState) {
    let size = f.area();

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(8),    // body
            Constraint::Length(1), // footer
        ])
        .split(size);

    // Title bar
    let title = Line::from(vec![
        Span::styled(
            " SCAVENGER OBSERVE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            if app.last_metrics.is_some() {
                "CONNECTED"
            } else {
                "DISCONNECTED"
            },
            Style::default().fg(if app.last_metrics.is_some() {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ]);
    f.render_widget(Paragraph::new(title), main_layout[0]);

    if let Some(ref err) = app.error {
        let error_block = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                err.as_str(),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Start the daemon with: scavenger daemon"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(error_block, main_layout[1]);
    } else if let Some(ref metrics) = app.last_metrics {
        render_dashboard(f, main_layout[1], metrics, &app.recent_log_lines);
    }

    // Footer
    let footer = Line::from(vec![
        Span::styled(" q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" refresh"),
    ]);
    f.render_widget(Paragraph::new(footer), main_layout[2]);
}

fn render_dashboard(f: &mut ratatui::Frame, area: Rect, metrics: &Value, log_lines: &[LogEntry]) {
    let body_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // top stats
            Constraint::Min(6),    // latency bars + log
        ])
        .split(area);

    // Top row: 4 stat panels
    let top_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(body_layout[0]);

    render_requests_panel(f, top_row[0], metrics);
    render_capsule_panel(f, top_row[1], metrics);
    render_graph_panel(f, top_row[2], metrics);
    render_daemon_panel(f, top_row[3], metrics);

    // Bottom row: latency chart + log stream
    let bottom_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(body_layout[1]);

    render_latency_chart(f, bottom_row[0], metrics);
    render_log_stream(f, bottom_row[1], log_lines);
}

fn render_requests_panel(f: &mut ratatui::Frame, area: Rect, m: &Value) {
    let req = &m["requests"];
    let total = req["total"].as_u64().unwrap_or(0);
    let rate = req["rate_per_min"].as_f64().unwrap_or(0.0);
    let p50 = req["latency_us"]["p50"].as_u64().unwrap_or(0);
    let p95 = req["latency_us"]["p95"].as_u64().unwrap_or(0);

    let lines = vec![
        Line::from(format!("  Total:  {total}")),
        Line::from(format!("  Rate:   {rate:.1}/min")),
        Line::from(format!("  P50:    {p50}us")),
        Line::from(format!("  P95:    {p95}us")),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Requests ")
        .title_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_capsule_panel(f: &mut ratatui::Frame, area: Rect, m: &Value) {
    let cap = &m["capsule"];
    let total = cap["total"].as_u64().unwrap_or(0);
    let empty = cap["empty"].as_u64().unwrap_or(0);
    let empty_rate = cap["empty_rate"].as_f64().unwrap_or(0.0) * 100.0;
    let avg_tokens = cap["tokens"]["avg"].as_u64().unwrap_or(0);
    let avg_util = cap["budget_utilization_pct"]["avg"].as_u64().unwrap_or(0);

    let lines = vec![
        Line::from(format!("  Served: {total}")),
        Line::from(format!("  Empty:  {empty} ({empty_rate:.0}%)")),
        Line::from(format!("  Tokens: ~{avg_tokens}")),
        Line::from(format!("  Budget: {avg_util}%")),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Capsules ")
        .title_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_graph_panel(f: &mut ratatui::Frame, area: Rect, m: &Value) {
    let graph = &m["graph"];
    let nodes = graph["nodes"].as_u64().unwrap_or(0);
    let edges = graph["edges"].as_u64().unwrap_or(0);
    let reindex = &m["reindex"];
    let ri_count = reindex["count"].as_u64().unwrap_or(0);
    let ri_p50 = reindex["latency_us"]["p50"].as_u64().unwrap_or(0);

    let lines = vec![
        Line::from(format!("  Nodes:   {nodes}")),
        Line::from(format!("  Edges:   {edges}")),
        Line::from(format!("  Reindex: {ri_count}x")),
        Line::from(format!("  Ri P50:  {ri_p50}us")),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Graph ")
        .title_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_daemon_panel(f: &mut ratatui::Frame, area: Rect, m: &Value) {
    let uptime = m["uptime_secs"].as_u64().unwrap_or(0);
    let errors = m["errors"].as_u64().unwrap_or(0);
    let hooks = &m["hooks"];
    let pre = hooks["pre_count"].as_u64().unwrap_or(0);
    let post = hooks["post_count"].as_u64().unwrap_or(0);
    let injected = hooks["pre_injected"].as_u64().unwrap_or(0);

    let uptime_str = if uptime >= 3600 {
        format!("{}h{}m", uptime / 3600, (uptime % 3600) / 60)
    } else if uptime >= 60 {
        format!("{}m{}s", uptime / 60, uptime % 60)
    } else {
        format!("{uptime}s")
    };

    let lines = vec![
        Line::from(format!("  Uptime:  {uptime_str}")),
        Line::from(format!("  Errors:  {errors}")),
        Line::from(format!("  Hooks:   {pre}pre/{post}post")),
        Line::from(format!("  Inject:  {injected}")),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Daemon ")
        .title_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_latency_chart(f: &mut ratatui::Frame, area: Rect, m: &Value) {
    let pipeline = &m["pipeline_us"];
    let stages = [
        ("gather", pipeline["gather"]["avg"].as_u64().unwrap_or(0)),
        ("score", pipeline["score"]["avg"].as_u64().unwrap_or(0)),
        ("pin", pipeline["pin"]["avg"].as_u64().unwrap_or(0)),
        ("trim", pipeline["trim"]["avg"].as_u64().unwrap_or(0)),
        ("group", pipeline["group"]["avg"].as_u64().unwrap_or(0)),
        ("render", pipeline["render"]["avg"].as_u64().unwrap_or(0)),
    ];

    let bars: Vec<Bar> = stages
        .iter()
        .map(|(name, val)| {
            Bar::default()
                .label(Line::from(*name))
                .value(*val)
                .style(Style::default().fg(Color::Cyan))
        })
        .collect();

    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Pipeline Avg (us) ")
                .title_style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .bar_width(5)
        .bar_gap(1)
        .data(BarGroup::default().bars(&bars));

    f.render_widget(chart, area);
}

fn render_log_stream(f: &mut ratatui::Frame, area: Rect, log_lines: &[LogEntry]) {
    let rows: Vec<Row> = log_lines
        .iter()
        .map(|entry| {
            let level_style = match entry.level.as_str() {
                "ERROR" => Style::default().fg(Color::Red),
                "WARN" => Style::default().fg(Color::Yellow),
                "INFO" => Style::default().fg(Color::Green),
                "DEBUG" => Style::default().fg(Color::Blue),
                _ => Style::default(),
            };

            let msg = if entry.fields.is_empty() {
                entry.message.clone()
            } else {
                format!("{} {}", entry.message, entry.fields)
            };

            Row::new(vec![
                ratatui::widgets::Cell::from(entry.time.clone()),
                ratatui::widgets::Cell::from(entry.level.clone()).style(level_style),
                ratatui::widgets::Cell::from(msg),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Min(20),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Recent Logs ")
            .title_style(Style::default().add_modifier(Modifier::BOLD)),
    );

    f.render_widget(table, area);
}
