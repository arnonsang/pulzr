use crate::stats::{LiveMetrics, StatsCollector};
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline},
    Terminal,
};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::{interval, Instant};

pub struct TuiApp {
    stats_collector: Arc<StatsCollector>,
    should_quit: bool,
    start_time: Instant,
    last_metrics: Option<LiveMetrics>,
    response_times: Vec<u64>,
    quit_sender: Option<broadcast::Sender<()>>,
}

impl TuiApp {
    pub fn new(stats_collector: Arc<StatsCollector>) -> Self {
        Self {
            stats_collector,
            should_quit: false,
            start_time: Instant::now(),
            last_metrics: None,
            response_times: Vec::new(),
            quit_sender: None,
        }
    }

    pub fn with_quit_sender(mut self, quit_sender: broadcast::Sender<()>) -> Self {
        self.quit_sender = Some(quit_sender);
        self
    }

    pub async fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut interval = interval(Duration::from_millis(250));

        loop {
            interval.tick().await;

            let metrics = self.stats_collector.get_live_metrics().await;
            self.last_metrics = Some(metrics.clone());

            if self.response_times.len() > 100 {
                self.response_times.remove(0);
            }
            if metrics.avg_response_time > 0.0 {
                self.response_times.push(metrics.avg_response_time as u64);
            }

            terminal.draw(|f| self.ui(f))?;

            if event::poll(Duration::from_millis(10))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') => {
                                self.should_quit = true;
                                if let Some(sender) = &self.quit_sender {
                                    let _ = sender.send(());
                                }
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.should_quit = true;
                                if let Some(sender) = &self.quit_sender {
                                    let _ = sender.send(());
                                }
                            }
                            KeyCode::Esc => {
                                self.should_quit = true;
                                if let Some(sender) = &self.quit_sender {
                                    let _ = sender.send(());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn ui(&self, f: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(6),
                Constraint::Length(8),
            ])
            .split(f.area());

        self.draw_header(f, chunks[0]);
        self.draw_metrics(f, chunks[1]);
        self.draw_response_chart(f, chunks[2]);
    }

    fn draw_header(&self, f: &mut ratatui::Frame, area: Rect) {
        let elapsed = self.start_time.elapsed();
        let header_text = format!(
            "Pulzr Load Tester - Running for {}m {}s | Press 'q' to quit",
            elapsed.as_secs() / 60,
            elapsed.as_secs() % 60
        );

        let header = Paragraph::new(header_text)
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::ALL));

        f.render_widget(header, area);
    }

    fn draw_metrics(&self, f: &mut ratatui::Frame, area: Rect) {
        let metrics = match &self.last_metrics {
            Some(m) => m,
            None => return,
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(4),
            ])
            .split(chunks[0]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(4),
            ])
            .split(chunks[1]);

        let requests_sent = Paragraph::new(format!("Requests Sent: {}", metrics.requests_sent))
            .style(Style::default().fg(Color::Green))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(requests_sent, left_chunks[0]);

        let rps = Paragraph::new(format!("RPS: {:.2}", metrics.current_rps))
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(rps, left_chunks[1]);

        let avg_response =
            Paragraph::new(format!("Avg Response: {:.2}ms", metrics.avg_response_time))
                .style(Style::default().fg(Color::Blue))
                .block(Block::default().borders(Borders::ALL));
        f.render_widget(avg_response, left_chunks[2]);

        let success_rate = if metrics.requests_sent > 0 {
            (metrics.requests_completed - metrics.requests_failed) as f64
                / metrics.requests_sent as f64
                * 100.0
        } else {
            0.0
        };

        let success_gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Success Rate"))
            .gauge_style(Style::default().fg(Color::Green))
            .ratio(success_rate / 100.0)
            .label(format!("{:.1}%", success_rate));
        f.render_widget(success_gauge, left_chunks[3]);

        let failed_requests = Paragraph::new(format!("Failed: {}", metrics.requests_failed))
            .style(Style::default().fg(Color::Red))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(failed_requests, right_chunks[0]);

        let min_max = Paragraph::new(format!(
            "Min: {}ms | Max: {}ms",
            metrics.min_response_time, metrics.max_response_time
        ))
        .style(Style::default().fg(Color::Magenta))
        .block(Block::default().borders(Borders::ALL));
        f.render_widget(min_max, right_chunks[1]);

        let bytes_received =
            Paragraph::new(format!("Bytes: {}", format_bytes(metrics.bytes_received)))
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL));
        f.render_widget(bytes_received, right_chunks[2]);

        let percentiles = Paragraph::new(format!(
            "P50: {}ms | P90: {}ms | P95: {}ms | P99: {}ms",
            metrics.p50_response_time,
            metrics.p90_response_time,
            metrics.p95_response_time,
            metrics.p99_response_time
        ))
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL).title("Percentiles"));
        f.render_widget(percentiles, right_chunks[3]);

        let status_items: Vec<ListItem> = metrics
            .status_codes
            .iter()
            .map(|(code, count)| {
                let color = match *code {
                    200..=299 => Color::Green,
                    300..=399 => Color::Yellow,
                    400..=499 => Color::Red,
                    500..=599 => Color::Magenta,
                    _ => Color::White,
                };
                ListItem::new(format!("{}: {}", code, count)).style(Style::default().fg(color))
            })
            .collect();

        let status_list = List::new(status_items)
            .block(Block::default().borders(Borders::ALL).title("Status Codes"));
        f.render_widget(status_list, right_chunks[4]);

        if !metrics.errors.is_empty() {
            let error_items: Vec<ListItem> = metrics
                .errors
                .iter()
                .take(5)
                .map(|(error, count)| {
                    let short_error = if error.len() > 40 {
                        format!("{}...", &error[..37])
                    } else {
                        error.clone()
                    };
                    ListItem::new(format!("{}: {}", short_error, count))
                        .style(Style::default().fg(Color::Red))
                })
                .collect();

            let error_list = List::new(error_items)
                .block(Block::default().borders(Borders::ALL).title("Errors"));
            f.render_widget(error_list, left_chunks[4]);
        }
    }

    fn draw_response_chart(&self, f: &mut ratatui::Frame, area: Rect) {
        if self.response_times.is_empty() {
            return;
        }

        let sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Response Time (ms)"),
            )
            .style(Style::default().fg(Color::Yellow))
            .data(&self.response_times);

        f.render_widget(sparkline, area);
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}
