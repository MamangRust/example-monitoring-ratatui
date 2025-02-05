use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{BarChart, Block, Borders, Gauge, Paragraph},
    Terminal,
};
use std::{io, time::Duration};
use sysinfo::{Networks, System};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut sys = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();

    let mut cpu_data = vec![0; 50];
    let mut ram_data = vec![0; 50];
    let mut network_receive_data = vec![0; 50];
    let mut network_send_data = vec![0; 50];

    loop {
        sys.refresh_all();
        networks.refresh();

        let mut cpu_tes: f32 = 0.0;

        for cpu in sys.cpus() {
            cpu_tes = cpu.cpu_usage();
        }

        let memory = sys.total_memory();
        let used_memory = sys.used_memory();
        let memory_usage_percentage = (used_memory as f64 / memory as f64) * 100.0;

        let mut total_received = 0;
        let mut total_transmitted = 0;
        for (_, network) in &networks {
            total_received += network.received();
            total_transmitted += network.transmitted();
        }

        cpu_data.remove(0);
        cpu_data.push(cpu_tes as u64);
        ram_data.remove(0);
        ram_data.push(memory_usage_percentage as u64);
        network_receive_data.remove(0);
        network_receive_data.push(total_received / 1024);
        network_send_data.remove(0);
        network_send_data.push(total_transmitted / 1024);

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Percentage(30),
                        Constraint::Percentage(30),
                        Constraint::Percentage(40),
                    ]
                    .as_ref(),
                )
                .split(f.area());

            let cpu_ram_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);

            let cpu_gauge = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("CPU Usage (%)"),
                )
                .gauge_style(Style::default().fg(Color::Blue))
                .percent(cpu_tes as u16);
            f.render_widget(cpu_gauge, cpu_ram_chunks[0]);

            let ram_gauge = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("RAM Usage (%)"),
                )
                .gauge_style(Style::default().fg(Color::Green))
                .percent(memory_usage_percentage as u16);
            f.render_widget(ram_gauge, cpu_ram_chunks[1]);

            let network_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);

            let network_receive =
                Paragraph::new(format!("Network Receive: {} KB", total_received / 1024)).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Network Receive"),
                );
            f.render_widget(network_receive, network_chunks[0]);

            let network_send =
                Paragraph::new(format!("Network Send: {} KB", total_transmitted / 1024))
                    .block(Block::default().borders(Borders::ALL).title("Network Send"));
            f.render_widget(network_send, network_chunks[1]);

            let graph_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[2]);

            let cpu_graph_data: Vec<(String, u64)> = cpu_data
                .iter()
                .enumerate()
                .map(|(i, &value)| (i.to_string(), value))
                .collect();

            let cput_graph_data_ref: Vec<(&str, u64)> = cpu_graph_data
                .iter()
                .map(|(label, value)| (label.as_str(), *value))
                .collect();

            let cpu_graph = BarChart::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("CPU Usage Over Time"),
                )
                .data(&cput_graph_data_ref)
                .bar_width(3)
                .bar_gap(1)
                .bar_style(Style::default().fg(Color::Blue))
                .value_style(Style::default().fg(Color::Black).bg(Color::Blue));
            f.render_widget(cpu_graph, graph_chunks[0]);

            let ram_graph_data: Vec<(String, u64)> = ram_data
                .iter()
                .enumerate()
                .map(|(i, &value)| (i.to_string(), value))
                .collect();

            let ram_graph_data_ref: Vec<(&str, u64)> = ram_graph_data
                .iter()
                .map(|(label, value)| (label.as_str(), *value))
                .collect();

            let ram_graph = BarChart::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("RAM Usage Over Time"),
                )
                .data(&ram_graph_data_ref)
                .bar_width(3)
                .bar_gap(1)
                .bar_style(Style::default().fg(Color::Green))
                .value_style(Style::default().fg(Color::Black).bg(Color::Green));
            f.render_widget(ram_graph, graph_chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
