use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        BarChart, Block, Borders, Cell, Gauge,  Paragraph, Row,
        Sparkline, Table, TableState, Tabs, Wrap,
    },
    Terminal,
};
use std::{
    collections::HashMap,
    io,
    process::Command,
    time::{Duration, Instant},
};
use sysinfo::{Networks, System};

#[derive(Clone, PartialEq)]
enum MonitorTab {
    System,
    Docker,
    Kubernetes,
}

#[derive(Clone)]
struct DockerContainer {
    id: String,
    image: String,
    name: String,
    status: String,
    ports: String,
    cpu_percent: f64,
    mem_usage: String,
    mem_percent: f64,
    net_io: String,
    block_io: String,
}

#[derive(Clone)]
struct DockerImage {
    repository: String,
    tag: String,
    image_id: String,
    size: String,
}

#[derive(Clone)]
struct K8sPod {
    name: String,
    namespace: String,
    status: String,
    restarts: String,
    age: String,
}

struct AppState {
    current_tab: MonitorTab,
    docker_list_state: TableState,
    docker_containers: Vec<DockerContainer>,
    docker_images: Vec<DockerImage>,
    k8s_pods: Vec<K8sPod>,
    k8s_list_state: TableState,
    docker_view: DockerView,
    message: String,
    show_create_dialog: bool,
    create_dialog_state: CreateDialogState,
    container_stats_history: HashMap<String, Vec<ContainerStats>>,
}

#[derive(Clone)]
struct ContainerStats {
    cpu_percent: f64,
    mem_percent: f64,
    net_rx_kb: f64,
    net_tx_kb: f64,
}

#[derive(Clone, PartialEq)]
enum DockerView {
    Containers,
    Images,
}

#[derive(Clone)]
struct CreateDialogState {
    selected_field: usize,
    image_name: String,
    container_name: String,
    ports: String,
    env_vars: String,
    volumes: String,
    command: String,
}

impl CreateDialogState {
    fn new() -> Self {
        Self {
            selected_field: 0,
            image_name: String::new(),
            container_name: String::new(),
            ports: String::new(),
            env_vars: String::new(),
            volumes: String::new(),
            command: String::new(),
        }
    }

    fn get_current_field_mut(&mut self) -> &mut String {
        match self.selected_field {
            0 => &mut self.image_name,
            1 => &mut self.container_name,
            2 => &mut self.ports,
            3 => &mut self.env_vars,
            4 => &mut self.volumes,
            5 => &mut self.command,
            _ => &mut self.image_name,
        }
    }
}

impl AppState {
    fn new() -> Self {
        let mut state = AppState {
            current_tab: MonitorTab::System,
            docker_list_state: TableState::default(),
            docker_containers: Vec::new(),
            docker_images: Vec::new(),
            k8s_pods: Vec::new(),
            k8s_list_state: TableState::default(),
            docker_view: DockerView::Containers,
            message: String::new(),
            show_create_dialog: false,
            create_dialog_state: CreateDialogState::new(),
            container_stats_history: HashMap::new(),
        };
        state.docker_list_state.select(Some(0));
        state.k8s_list_state.select(Some(0));
        state
    }

    fn next_docker_item(&mut self) {
        let items_len = match self.docker_view {
            DockerView::Containers => self.docker_containers.len(),
            DockerView::Images => self.docker_images.len(),
        };

        if items_len == 0 {
            return;
        }

        let i = match self.docker_list_state.selected() {
            Some(i) => {
                if i >= items_len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.docker_list_state.select(Some(i));
    }

    fn previous_docker_item(&mut self) {
        let items_len = match self.docker_view {
            DockerView::Containers => self.docker_containers.len(),
            DockerView::Images => self.docker_images.len(),
        };

        if items_len == 0 {
            return;
        }

        let i = match self.docker_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    items_len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.docker_list_state.select(Some(i));
    }

    fn next_k8s_item(&mut self) {
        if self.k8s_pods.is_empty() {
            return;
        }

        let i = match self.k8s_list_state.selected() {
            Some(i) => {
                if i >= self.k8s_pods.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.k8s_list_state.select(Some(i));
    }

    fn previous_k8s_item(&mut self) {
        if self.k8s_pods.is_empty() {
            return;
        }

        let i = match self.k8s_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.k8s_pods.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.k8s_list_state.select(Some(i));
    }

    fn update_container_stats(&mut self, container_id: &str, stats: ContainerStats) {
        let history = self
            .container_stats_history
            .entry(container_id.to_string())
            .or_insert_with(Vec::new);

        history.push(stats);
        if history.len() > 60 {
            history.remove(0);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut sys = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();

    let mut cpu_data = vec![0; 60];
    let mut ram_data = vec![0; 60];
    let mut network_receive_speed_data = vec![0; 60];
    let mut network_send_speed_data = vec![0; 60];

    let mut prev_total_received = None;
    let mut prev_total_transmitted = None;
    let mut last_update = Instant::now();

    let mut app_state = AppState::new();

    loop {
        sys.refresh_all();
        networks.refresh();

        let cpu_usage = sys.global_cpu_usage();
        let memory = sys.total_memory();
        let used_memory = sys.used_memory();
        let memory_usage_percentage = (used_memory as f64 / memory as f64) * 100.0;

        let mut total_received = 0;
        let mut total_transmitted = 0;
        for (_, network) in &networks {
            total_received += network.received();
            total_transmitted += network.transmitted();
        }

        let elapsed_secs = last_update.elapsed().as_secs_f64().max(1e-6);
        let mut receive_rate_kbs = 0.0;
        let mut send_rate_kbs = 0.0;

        if let (Some(prev_recv), Some(prev_send)) = (prev_total_received, prev_total_transmitted) {
            let diff_recv = total_received.saturating_sub(prev_recv);
            let diff_send = total_transmitted.saturating_sub(prev_send);
            receive_rate_kbs = diff_recv as f64 / 1024.0 / elapsed_secs;
            send_rate_kbs = diff_send as f64 / 1024.0 / elapsed_secs;
        }

        prev_total_received = Some(total_received);
        prev_total_transmitted = Some(total_transmitted);
        last_update = Instant::now();

        cpu_data.remove(0);
        cpu_data.push(cpu_usage.round() as u64);
        ram_data.remove(0);
        ram_data.push(memory_usage_percentage as u64);
        network_receive_speed_data.remove(0);
        network_receive_speed_data.push(receive_rate_kbs.round() as u64);
        network_send_speed_data.remove(0);
        network_send_speed_data.push(send_rate_kbs.round() as u64);

        match app_state.current_tab {
            MonitorTab::Docker => {
                app_state.docker_containers = get_docker_containers_with_stats();

                let mut updates = Vec::new();
                for container in &app_state.docker_containers {
                    let stats = ContainerStats {
                        cpu_percent: container.cpu_percent,
                        mem_percent: container.mem_percent,
                        net_rx_kb: parse_net_io(&container.net_io).0,
                        net_tx_kb: parse_net_io(&container.net_io).1,
                    };
                    updates.push((container.id.clone(), stats));
                }

                for (id, stats) in updates {
                    app_state.update_container_stats(&id, stats);
                }

                app_state.docker_images = get_docker_images();
            }

            MonitorTab::Kubernetes => {
                app_state.k8s_pods = get_k8s_pods();
            }

            _ => {}
        }

        terminal.draw(|f| {
            let size = f.area();

            if app_state.show_create_dialog {
                render_create_dialog(f, size, &app_state.create_dialog_state);
                return;
            }

            let tabs_block = Block::default().borders(Borders::ALL).title("Tabs");
            let tabs = Tabs::new(vec![
                "System (Ctrl+S)",
                "Docker (Ctrl+D)",
                "Kubernetes (Ctrl+K)",
            ])
            .block(tabs_block)
            .select(match app_state.current_tab {
                MonitorTab::System => 0,
                MonitorTab::Docker => 1,
                MonitorTab::Kubernetes => 2,
            })
            .style(Style::default().fg(Color::White))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                .split(size);

            f.render_widget(tabs, chunks[0]);

            match app_state.current_tab {
                MonitorTab::System => render_system_tab(
                    f,
                    chunks[1],
                    &sys,
                    &cpu_data,
                    &ram_data,
                    &network_receive_speed_data,
                    &network_send_speed_data,
                    cpu_usage,
                    memory,
                    used_memory,
                    memory_usage_percentage,
                    receive_rate_kbs,
                    send_rate_kbs,
                    total_received,
                    total_transmitted,
                ),
                MonitorTab::Docker => render_docker_tab(f, chunks[1], &mut app_state),
                MonitorTab::Kubernetes => render_k8s_tab(f, chunks[1], &mut app_state),
            }
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if app_state.show_create_dialog {
                    match key.code {
                        KeyCode::Esc => {
                            app_state.show_create_dialog = false;
                            app_state.create_dialog_state = CreateDialogState::new();
                        }
                        KeyCode::Tab => {
                            app_state.create_dialog_state.selected_field =
                                (app_state.create_dialog_state.selected_field + 1) % 6;
                        }
                        KeyCode::BackTab => {
                            app_state.create_dialog_state.selected_field =
                                if app_state.create_dialog_state.selected_field == 0 {
                                    5
                                } else {
                                    app_state.create_dialog_state.selected_field - 1
                                };
                        }
                        KeyCode::Char(c) => {
                            app_state
                                .create_dialog_state
                                .get_current_field_mut()
                                .push(c);
                        }
                        KeyCode::Backspace => {
                            app_state.create_dialog_state.get_current_field_mut().pop();
                        }
                        KeyCode::Enter => {
                            let result = create_custom_container(&app_state.create_dialog_state);
                            app_state.message = result;
                            app_state.show_create_dialog = false;
                            app_state.create_dialog_state = CreateDialogState::new();
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app_state.current_tab = MonitorTab::System;
                        }
                        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app_state.current_tab = MonitorTab::Docker;
                        }
                        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app_state.current_tab = MonitorTab::Kubernetes;
                        }
                        KeyCode::Down => match app_state.current_tab {
                            MonitorTab::Docker => app_state.next_docker_item(),
                            MonitorTab::Kubernetes => app_state.next_k8s_item(),
                            _ => {}
                        },
                        KeyCode::Up => match app_state.current_tab {
                            MonitorTab::Docker => app_state.previous_docker_item(),
                            MonitorTab::Kubernetes => app_state.previous_k8s_item(),
                            _ => {}
                        },
                        KeyCode::Tab if app_state.current_tab == MonitorTab::Docker => {
                            app_state.docker_view = match app_state.docker_view {
                                DockerView::Containers => DockerView::Images,
                                DockerView::Images => DockerView::Containers,
                            };
                            app_state.docker_list_state.select(Some(0));
                        }
                        KeyCode::Char('n') if app_state.current_tab == MonitorTab::Docker => {
                            app_state.show_create_dialog = true;
                            app_state.create_dialog_state = CreateDialogState::new();
                        }
                        KeyCode::Char('p') if app_state.current_tab == MonitorTab::Docker => {
                            create_postgres_container();
                            app_state.message = "Creating PostgreSQL container...".to_string();
                        }
                        KeyCode::Char('r') if app_state.current_tab == MonitorTab::Docker => {
                            create_redis_container();
                            app_state.message = "Creating Redis container...".to_string();
                        }
                        KeyCode::Char('m') if app_state.current_tab == MonitorTab::Docker => {
                            create_mongodb_container();
                            app_state.message = "Creating MongoDB container...".to_string();
                        }
                        KeyCode::Char('g') if app_state.current_tab == MonitorTab::Docker => {
                            create_grafana_container();
                            app_state.message = "Creating Grafana container...".to_string();
                        }
                        KeyCode::Char('x') if app_state.current_tab == MonitorTab::Docker => {
                            if let Some(selected) = app_state.docker_list_state.selected() {
                                match app_state.docker_view {
                                    DockerView::Containers => {
                                        if let Some(container) =
                                            app_state.docker_containers.get(selected)
                                        {
                                            stop_docker_container(&container.id);
                                            app_state.message =
                                                format!("Stopped container: {}", container.name);
                                        }
                                    }
                                    DockerView::Images => {
                                        if let Some(image) = app_state.docker_images.get(selected) {
                                            delete_docker_image(&image.image_id);
                                            app_state.message =
                                                format!("Deleted image: {}", image.repository);
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('s')
                            if app_state.current_tab == MonitorTab::Docker
                                && app_state.docker_view == DockerView::Containers =>
                        {
                            if let Some(selected) = app_state.docker_list_state.selected() {
                                if let Some(container) = app_state.docker_containers.get(selected) {
                                    start_docker_container(&container.id);
                                    app_state.message =
                                        format!("Started container: {}", container.name);
                                }
                            }
                        }
                        KeyCode::Char('t')
                            if app_state.current_tab == MonitorTab::Docker
                                && app_state.docker_view == DockerView::Containers =>
                        {
                            if let Some(selected) = app_state.docker_list_state.selected() {
                                if let Some(container) = app_state.docker_containers.get(selected) {
                                    restart_docker_container(&container.id);
                                    app_state.message =
                                        format!("Restarting container: {}", container.name);
                                }
                            }
                        }
                        KeyCode::Delete
                            if app_state.current_tab == MonitorTab::Docker
                                && app_state.docker_view == DockerView::Containers =>
                        {
                            if let Some(selected) = app_state.docker_list_state.selected() {
                                if let Some(container) = app_state.docker_containers.get(selected) {
                                    delete_docker_container(&container.id);
                                    app_state.message =
                                        format!("Deleted container: {}", container.name);
                                }
                            }
                        }
                        KeyCode::Char('d') if app_state.current_tab == MonitorTab::Kubernetes => {
                            if let Some(selected) = app_state.k8s_list_state.selected() {
                                if let Some(pod) = app_state.k8s_pods.get(selected) {
                                    delete_k8s_pod(&pod.name, &pod.namespace);
                                    app_state.message = format!("Deleted pod: {}", pod.name);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn render_system_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    sys: &System,
    cpu_data: &[u64],
    ram_data: &[u64],
    network_receive_speed_data: &[u64],
    network_send_speed_data: &[u64],
    cpu_usage: f32,
    memory: u64,
    used_memory: u64,
    memory_usage_percentage: f64,
    receive_rate_kbs: f64,
    send_rate_kbs: f64,
    total_received: u64,
    total_transmitted: u64,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(6),
                Constraint::Percentage(28),
                Constraint::Length(6),
                Constraint::Percentage(60),
            ]
            .as_ref(),
        )
        .split(area);

    let load = System::load_average();
    let uptime =
        format_duration(Duration::from_secs(System::uptime())).unwrap_or_else(|| "N/A".to_string());
    let host_name = System::host_name().unwrap_or_else(|| "Unknown host".to_string());
    let summary_lines = vec![
        Line::from(vec![Span::styled(
            "System Overview",
            Style::default().fg(Color::Yellow),
        )]),
        Line::from(format!(
            "Host: {}  |  CPUs: {}  |  Uptime: {}",
            host_name,
            sys.cpus().len(),
            uptime
        )),
        Line::from(format!(
            "Load Avg (1/5/15m): {:.2} / {:.2} / {:.2}  |  Press 'q' to quit",
            load.one, load.five, load.fifteen
        )),
    ];
    let summary = Paragraph::new(summary_lines)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(summary, chunks[0]);

    let cpu_ram_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let cpu_percent = clamp_percent(cpu_usage);
    let cpu_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("CPU Usage (%)"),
        )
        .label(format!("{:.1}%", cpu_usage))
        .gauge_style(Style::default().fg(Color::Blue))
        .percent(cpu_percent);
    f.render_widget(cpu_gauge, cpu_ram_chunks[0]);

    let memory_label = format!(
        "{:.1}% ({:.1} / {:.1} GiB)",
        memory_usage_percentage,
        kib_to_gib(used_memory as f64),
        kib_to_gib(memory as f64)
    );
    let ram_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("RAM Usage (%)"),
        )
        .label(memory_label)
        .gauge_style(Style::default().fg(Color::Green))
        .percent(memory_usage_percentage as u16);
    f.render_widget(ram_gauge, cpu_ram_chunks[1]);

    let network_info = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            "Network",
            Style::default().fg(Color::Cyan),
        )]),
        Line::from(format!(
            "Download: {:>7.1} KB/s  |  Upload: {:>7.1} KB/s",
            receive_rate_kbs, send_rate_kbs
        )),
        Line::from(format!(
            "Total Received: {:>8.2} MiB  |  Total Sent: {:>8.2} MiB",
            total_received as f64 / 1024.0 / 1024.0,
            total_transmitted as f64 / 1024.0 / 1024.0
        )),
    ])
    .block(Block::default().borders(Borders::ALL))
    .wrap(Wrap { trim: true });
    f.render_widget(network_info, chunks[2]);

    let graph_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[3]);

    let cpu_graph_data: Vec<(String, u64)> = cpu_data
        .iter()
        .enumerate()
        .map(|(i, &value)| (i.to_string(), value))
        .collect();

    let cpu_graph_data_ref: Vec<(&str, u64)> = cpu_graph_data
        .iter()
        .map(|(label, value)| (label.as_str(), *value))
        .collect();

    let cpu_graph = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("CPU Usage Over Time"),
        )
        .data(&cpu_graph_data_ref)
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

    let right_graph_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(graph_chunks[1]);

    f.render_widget(ram_graph, right_graph_chunks[0]);

    let spark_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right_graph_chunks[1]);

    let download_sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Download KB/s"),
        )
        .style(Style::default().fg(Color::Cyan))
        .data(network_receive_speed_data);
    f.render_widget(download_sparkline, spark_chunks[0]);

    let upload_sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::ALL).title("Upload KB/s"))
        .style(Style::default().fg(Color::Magenta))
        .data(network_send_speed_data);
    f.render_widget(upload_sparkline, spark_chunks[1]);
}

fn render_docker_tab(f: &mut ratatui::Frame, area: Rect, app_state: &mut AppState) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(5)].as_ref())
        .split(area);

    let view_title = match app_state.docker_view {
        DockerView::Containers => {
            "Docker Containers | Tab:Switch | ↑↓:Nav | N:New | P:Postgres | R:Redis | M:Mongo | G:Grafana | S:Start | X:Stop | T:Restart | Del:Remove"
        }
        DockerView::Images => "Docker Images | Tab:Switch | ↑↓:Nav | X:Delete",
    };

    match app_state.docker_view {
        DockerView::Containers => {
            let has_selection = app_state.docker_list_state.selected().is_some();

            if has_selection && !app_state.docker_containers.is_empty() {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(main_chunks[0]);

                let rows: Vec<Row> = app_state
                    .docker_containers
                    .iter()
                    .map(|c| {
                        let status_color = if c.status.contains("Up") {
                            Color::Green
                        } else {
                            Color::Red
                        };

                        Row::new(vec![
                            Cell::from(c.id[..12].to_string()),
                            Cell::from(c.name.clone()),
                            Cell::from(c.image.clone()),
                            Cell::from(Span::styled(
                                c.status.clone(),
                                Style::default().fg(status_color),
                            )),
                            Cell::from(format!("{:.1}%", c.cpu_percent)),
                            Cell::from(c.mem_usage.clone()),
                        ])
                    })
                    .collect();

                let table = Table::new(
                    rows,
                    [
                        Constraint::Length(13),
                        Constraint::Length(20),
                        Constraint::Length(20),
                        Constraint::Length(15),
                        Constraint::Length(8),
                        Constraint::Min(10),
                    ],
                )
                .header(
                    Row::new(vec!["ID", "Name", "Image", "Status", "CPU%", "Mem"]).style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(Block::default().borders(Borders::ALL).title(view_title))
                .highlight_style(Style::default().bg(Color::DarkGray))
                .highlight_symbol(">> ");

                f.render_stateful_widget(table, chunks[0], &mut app_state.docker_list_state);

                if let Some(selected) = app_state.docker_list_state.selected() {
                    if let Some(container) = app_state.docker_containers.get(selected) {
                        render_container_stats(f, chunks[1], container, app_state);
                    }
                }
            } else {
                let rows: Vec<Row> = app_state
                    .docker_containers
                    .iter()
                    .map(|c| {
                        let status_color = if c.status.contains("Up") {
                            Color::Green
                        } else {
                            Color::Red
                        };

                        Row::new(vec![
                            Cell::from(c.id[..12].to_string()),
                            Cell::from(c.name.clone()),
                            Cell::from(c.image.clone()),
                            Cell::from(Span::styled(
                                c.status.clone(),
                                Style::default().fg(status_color),
                            )),
                            Cell::from(format!("{:.1}%", c.cpu_percent)),
                            Cell::from(c.mem_usage.clone()),
                            Cell::from(c.net_io.clone()),
                            Cell::from(c.block_io.clone()),
                        ])
                    })
                    .collect();

                let table = Table::new(
                    rows,
                    [
                        Constraint::Length(13),
                        Constraint::Length(20),
                        Constraint::Length(20),
                        Constraint::Length(15),
                        Constraint::Length(8),
                        Constraint::Length(15),
                        Constraint::Length(15),
                        Constraint::Min(10),
                    ],
                )
                .header(
                    Row::new(vec![
                        "ID",
                        "Name",
                        "Image",
                        "Status",
                        "CPU%",
                        "Mem",
                        "Net I/O",
                        "Block I/O",
                    ])
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(Block::default().borders(Borders::ALL).title(view_title))
                .highlight_style(Style::default().bg(Color::DarkGray))
                .highlight_symbol(">> ");

                f.render_stateful_widget(table, main_chunks[0], &mut app_state.docker_list_state);
            }
        }
        DockerView::Images => {
            let rows: Vec<Row> = app_state
                .docker_images
                .iter()
                .map(|img| {
                    Row::new(vec![
                        Cell::from(img.repository.clone()),
                        Cell::from(img.tag.clone()),
                        Cell::from(img.image_id[..12].to_string()),
                        Cell::from(img.size.clone()),
                    ])
                })
                .collect();

            let table = Table::new(
                rows,
                [
                    Constraint::Length(30),
                    Constraint::Length(15),
                    Constraint::Length(15),
                    Constraint::Min(15),
                ],
            )
            .header(
                Row::new(vec!["Repository", "Tag", "Image ID", "Size"]).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(Block::default().borders(Borders::ALL).title(view_title))
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol(">> ");

            f.render_stateful_widget(table, main_chunks[0], &mut app_state.docker_list_state);
        }
    }

    let help = Paragraph::new(app_state.message.clone())
        .block(Block::default().borders(Borders::ALL).title("Message"))
        .wrap(Wrap { trim: true });
    f.render_widget(help, main_chunks[1]);
}

fn render_container_stats(
    f: &mut ratatui::Frame,
    area: Rect,
    container: &DockerContainer,
    app_state: &AppState,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    // Container info
    let info = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            "Container Details",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!("Name: {}", container.name)),
        Line::from(format!("Ports: {}", container.ports)),
    ])
    .block(Block::default().borders(Borders::ALL))
    .wrap(Wrap { trim: true });
    f.render_widget(info, chunks[0]);


    if let Some(history) = app_state.container_stats_history.get(&container.id) {
        let cpu_data: Vec<u64> = history.iter().map(|s| s.cpu_percent as u64).collect();
        let cpu_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("CPU History ({:.1}%)", container.cpu_percent)),
            )
            .style(Style::default().fg(Color::Blue))
            .data(&cpu_data);
        f.render_widget(cpu_sparkline, chunks[1]);

        let mem_data: Vec<u64> = history.iter().map(|s| s.mem_percent as u64).collect();
        let mem_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Memory History ({:.1}%)", container.mem_percent)),
            )
            .style(Style::default().fg(Color::Green))
            .data(&mem_data);
        f.render_widget(mem_sparkline, chunks[2]);

        let net_data: Vec<u64> = history
            .iter()
            .map(|s| (s.net_rx_kb + s.net_tx_kb) as u64)
            .collect();
        let net_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Network I/O ({})", container.net_io)),
            )
            .style(Style::default().fg(Color::Cyan))
            .data(&net_data);
        f.render_widget(net_sparkline, chunks[3]);
    }
}

fn render_create_dialog(f: &mut ratatui::Frame, area: Rect, state: &CreateDialogState) {
    let popup_area = centered_rect(70, 80, area);

    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .margin(1)
        .split(popup_area);

    let block = Block::default()
        .title("Create Docker Container (Tab/Shift+Tab: Navigate, Enter: Create, Esc: Cancel)")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));
    f.render_widget(block, popup_area);

    let fields = [
        ("Image Name*", &state.image_name),
        ("Container Name", &state.container_name),
        ("Ports (e.g. 8080:80,443:443)", &state.ports),
        ("Env Vars (e.g. KEY=value,KEY2=value2)", &state.env_vars),
        ("Volumes (e.g. /host:/container)", &state.volumes),
        ("Command (optional)", &state.command),
    ];

    for (i, (label, value)) in fields.iter().enumerate() {
        let style = if i == state.selected_field {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let input = Paragraph::new(value.as_str()).style(style).block(
            Block::default()
                .borders(Borders::ALL)
                .title(*label)
                .border_style(style),
        );
        f.render_widget(input, chunks[i]);
    }

    let help_text = "Examples: postgres:latest, redis:alpine, nginx:latest, mysql:8.0";
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[6]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn render_k8s_tab(f: &mut ratatui::Frame, area: Rect, app_state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(4)].as_ref())
        .split(area);

    let rows: Vec<Row> = app_state
        .k8s_pods
        .iter()
        .map(|pod| {
            let status_color = match pod.status.as_str() {
                "Running" => Color::Green,
                "Pending" => Color::Yellow,
                "Failed" | "CrashLoopBackOff" => Color::Red,
                _ => Color::White,
            };

            Row::new(vec![
                Cell::from(pod.name.clone()),
                Cell::from(pod.namespace.clone()),
                Cell::from(Span::styled(
                    pod.status.clone(),
                    Style::default().fg(status_color),
                )),
                Cell::from(pod.restarts.clone()),
                Cell::from(pod.age.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(35),
            Constraint::Length(20),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["Name", "Namespace", "Status", "Restarts", "Age"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Kubernetes Pods (↑↓ Navigate | D Delete)"),
    )
    .highlight_style(Style::default().bg(Color::DarkGray))
    .highlight_symbol(">> ");

    f.render_stateful_widget(table, chunks[0], &mut app_state.k8s_list_state);

    let help = Paragraph::new(app_state.message.clone())
        .block(Block::default().borders(Borders::ALL).title("Message"))
        .wrap(Wrap { trim: true });
    f.render_widget(help, chunks[1]);
}

// Docker functions
fn get_docker_containers_with_stats() -> Vec<DockerContainer> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--format",
            "{{.ID}}|{{.Image}}|{{.Names}}|{{.Status}}|{{.Ports}}",
        ])
        .output();

    let containers_basic = match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split('|').collect();
                    if parts.len() >= 5 {
                        Some((
                            parts[0].to_string(),
                            parts[1].to_string(),
                            parts[2].to_string(),
                            parts[3].to_string(),
                            parts[4].to_string(),
                        ))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        }
        _ => return Vec::new(),
    };

    // Get stats
    let stats_output = Command::new("docker")
        .args([
            "stats",
            "--no-stream",
            "--format",
            "{{.ID}}|{{.CPUPerc}}|{{.MemUsage}}|{{.MemPerc}}|{{.NetIO}}|{{.BlockIO}}",
        ])
        .output();

    let mut stats_map: HashMap<String, (f64, String, f64, String, String)> = HashMap::new();

    if let Ok(output) = stats_output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 6 {
                    let id = parts[0].to_string();
                    let cpu = parts[1].trim_end_matches('%').parse::<f64>().unwrap_or(0.0);
                    let mem_usage = parts[2].to_string();
                    let mem_percent = parts[3].trim_end_matches('%').parse::<f64>().unwrap_or(0.0);
                    let net_io = parts[4].to_string();
                    let block_io = parts[5].to_string();
                    stats_map.insert(id, (cpu, mem_usage, mem_percent, net_io, block_io));
                }
            }
        }
    }

    containers_basic
        .into_iter()
        .map(|(id, image, name, status, ports)| {
            let (cpu_percent, mem_usage, mem_percent, net_io, block_io) =
                stats_map.get(&id).cloned().unwrap_or((
                    0.0,
                    "N/A".to_string(),
                    0.0,
                    "N/A".to_string(),
                    "N/A".to_string(),
                ));

            DockerContainer {
                id,
                image,
                name,
                status,
                ports,
                cpu_percent,
                mem_usage,
                mem_percent,
                net_io,
                block_io,
            }
        })
        .collect()
}

fn parse_net_io(net_io: &str) -> (f64, f64) {
    let parts: Vec<&str> = net_io.split('/').map(|s| s.trim()).collect();
    if parts.len() != 2 {
        return (0.0, 0.0);
    }

    let parse_value = |s: &str| -> f64 {
        let s = s.trim();
        if let Some(num_str) = s.strip_suffix("kB") {
            num_str.parse::<f64>().unwrap_or(0.0)
        } else if let Some(num_str) = s.strip_suffix("MB") {
            num_str.parse::<f64>().unwrap_or(0.0) * 1024.0
        } else if let Some(num_str) = s.strip_suffix("GB") {
            num_str.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0
        } else {
            0.0
        }
    };

    (parse_value(parts[0]), parse_value(parts[1]))
}

fn get_docker_images() -> Vec<DockerImage> {
    let output = Command::new("docker")
        .args([
            "images",
            "--format",
            "{{.Repository}}|{{.Tag}}|{{.ID}}|{{.Size}}",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split('|').collect();
                    if parts.len() >= 4 {
                        Some(DockerImage {
                            repository: parts[0].to_string(),
                            tag: parts[1].to_string(),
                            image_id: parts[2].to_string(),
                            size: parts[3].to_string(),
                        })
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn create_custom_container(state: &CreateDialogState) -> String {
    if state.image_name.is_empty() {
        return "Error: Image name is required!".to_string();
    }

    let mut args = vec!["run", "-d"];

    // Container name
    if !state.container_name.is_empty() {
        args.push("--name");
        args.push(&state.container_name);
    }

    // Ports
    let port_args: Vec<String> = if !state.ports.is_empty() {
        state
            .ports
            .split(',')
            .flat_map(|p| vec!["-p".to_string(), p.trim().to_string()])
            .collect()
    } else {
        Vec::new()
    };

    // Env vars
    let env_args: Vec<String> = if !state.env_vars.is_empty() {
        state
            .env_vars
            .split(',')
            .flat_map(|e| vec!["-e".to_string(), e.trim().to_string()])
            .collect()
    } else {
        Vec::new()
    };

    // Volumes
    let vol_args: Vec<String> = if !state.volumes.is_empty() {
        state
            .volumes
            .split(',')
            .flat_map(|v| vec!["-v".to_string(), v.trim().to_string()])
            .collect()
    } else {
        Vec::new()
    };

    let port_refs: Vec<&str> = port_args.iter().map(|s| s.as_str()).collect();
    let env_refs: Vec<&str> = env_args.iter().map(|s| s.as_str()).collect();
    let vol_refs: Vec<&str> = vol_args.iter().map(|s| s.as_str()).collect();

    args.extend(port_refs);
    args.extend(env_refs);
    args.extend(vol_refs);
    args.push(&state.image_name);

    // Command
    let cmd_args: Vec<String> = if !state.command.is_empty() {
        state
            .command
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    let cmd_refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
    args.extend(cmd_refs);

    match Command::new("docker").args(&args).output() {
        Ok(output) => {
            if output.status.success() {
                format!(
                    "Container created successfully from image: {}",
                    state.image_name
                )
            } else {
                let error = String::from_utf8_lossy(&output.stderr);
                format!("Error creating container: {}", error)
            }
        }
        Err(e) => format!("Failed to execute docker command: {}", e),
    }
}

fn create_postgres_container() {
    Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            &format!("postgres-{}", chrono::Utc::now().timestamp()),
            "-e",
            "POSTGRES_PASSWORD=password",
            "-p",
            "5432:5432",
            "postgres:latest",
        ])
        .spawn()
        .ok();
}

fn create_redis_container() {
    Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            &format!("redis-{}", chrono::Utc::now().timestamp()),
            "-p",
            "6379:6379",
            "redis:latest",
        ])
        .spawn()
        .ok();
}

fn create_mongodb_container() {
    Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            &format!("mongodb-{}", chrono::Utc::now().timestamp()),
            "-e",
            "MONGO_INITDB_ROOT_USERNAME=admin",
            "-e",
            "MONGO_INITDB_ROOT_PASSWORD=password",
            "-p",
            "27017:27017",
            "mongo:latest",
        ])
        .spawn()
        .ok();
}

fn create_grafana_container() {
    Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            &format!("grafana-{}", chrono::Utc::now().timestamp()),
            "-p",
            "3000:3000",
            "grafana/grafana:latest",
        ])
        .spawn()
        .ok();
}

fn stop_docker_container(container_id: &str) {
    Command::new("docker")
        .args(["stop", container_id])
        .spawn()
        .ok();
}

fn start_docker_container(container_id: &str) {
    Command::new("docker")
        .args(["start", container_id])
        .spawn()
        .ok();
}

fn restart_docker_container(container_id: &str) {
    Command::new("docker")
        .args(["restart", container_id])
        .spawn()
        .ok();
}

fn delete_docker_container(container_id: &str) {
    Command::new("docker")
        .args(["rm", "-f", container_id])
        .spawn()
        .ok();
}

fn delete_docker_image(image_id: &str) {
    Command::new("docker")
        .args(["rmi", "-f", image_id])
        .spawn()
        .ok();
}

// Kubernetes functions
fn get_k8s_pods() -> Vec<K8sPod> {
    let output = Command::new("kubectl")
        .args([
            "get",
            "pods",
            "--all-namespaces",
            "-o",
            "custom-columns=NAME:.metadata.name,NAMESPACE:.metadata.namespace,STATUS:.status.phase,RESTARTS:.status.containerStatuses[0].restartCount,AGE:.metadata.creationTimestamp",
            "--no-headers",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 {
                        Some(K8sPod {
                            name: parts[0].to_string(),
                            namespace: parts[1].to_string(),
                            status: parts[2].to_string(),
                            restarts: parts[3].to_string(),
                            age: parts[4].to_string(),
                        })
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn delete_k8s_pod(pod_name: &str, namespace: &str) {
    Command::new("kubectl")
        .args(["delete", "pod", pod_name, "-n", namespace])
        .spawn()
        .ok();
}

// Helper functions
fn kib_to_gib(kib: f64) -> f64 {
    kib / 1024.0 / 1024.0
}

fn format_duration(duration: Duration) -> Option<String> {
    let total_seconds = duration.as_secs();
    let days = total_seconds / 86_400;
    let hours = (total_seconds % 86_400) / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    Some(match (days, hours, minutes) {
        (d, _, _) if d > 0 => format!("{}d {:02}h {:02}m", d, hours, minutes),
        (_, h, _) if h > 0 => format!("{:02}h {:02}m", h, minutes),
        (_, _, m) if m > 0 => format!("{:02}m {:02}s", m, seconds),
        _ => format!("{:02}s", seconds),
    })
}

fn clamp_percent(value: f32) -> u16 {
    value.round().clamp(0.0, 100.0) as u16
}
