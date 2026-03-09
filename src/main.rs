use flotilla_core::config::ConfigStore;
use flotilla_core::daemon::DaemonHandle;
use flotilla_core::in_process::InProcessDaemon;
use flotilla_tui::app;
use flotilla_tui::event;
use flotilla_tui::event_log;
use flotilla_tui::event_log::LevelExt;
use flotilla_tui::socket::SocketDaemon;
use flotilla_tui::ui;

use clap::Parser;
use color_eyre::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use std::io::stdout;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Flotilla: TUI dashboard for managing development workspaces
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Git repo roots (repeatable; auto-detected from cwd if omitted)
    #[arg(long)]
    repo_root: Vec<PathBuf>,

    /// Config directory
    #[arg(long)]
    config_dir: Option<PathBuf>,

    /// Socket path (default: ${config_dir}/flotilla.sock)
    #[arg(long)]
    socket: Option<PathBuf>,

    /// Run in embedded mode (no daemon, in-process)
    #[arg(long)]
    embedded: bool,

    #[command(subcommand)]
    command: Option<SubCommand>,
}

#[derive(clap::Subcommand)]
enum SubCommand {
    /// Run the daemon server
    Daemon {
        /// Idle timeout in seconds (0 = no timeout)
        #[arg(long, default_value = "300")]
        timeout: u64,
    },
    /// Print repo list and state
    Status,
    /// Stream daemon events to stdout
    Watch,
}

impl Cli {
    fn config_dir(&self) -> PathBuf {
        self.config_dir
            .clone()
            .unwrap_or_else(flotilla_core::config::flotilla_config_dir)
    }

    fn socket_path(&self) -> PathBuf {
        self.socket
            .clone()
            .unwrap_or_else(|| self.config_dir().join("flotilla.sock"))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    match &cli.command {
        Some(SubCommand::Daemon { timeout }) => run_daemon(&cli, *timeout).await,
        Some(SubCommand::Status) => run_status(&cli).await,
        Some(SubCommand::Watch) => run_watch(&cli).await,
        None => run_tui(cli).await,
    }
}

async fn run_tui(cli: Cli) -> Result<()> {
    event_log::init();
    let startup = std::time::Instant::now();
    let config = Arc::new(ConfigStore::new());

    // Initialize terminal and show splash immediately for fast visual feedback.
    // Mouse capture is enabled AFTER the splash so mouse events don't cut it short.
    let mut terminal = ratatui::init();

    // Resolve repos before splash (fast — just reads config files).
    let embedded = cli.embedded;
    let repo_roots = if embedded {
        let roots = flotilla_core::config::resolve_repo_roots(&cli.repo_root, &config);
        if roots.is_empty() {
            ratatui::restore();
            eprintln!("Error: no git repositories found (use --repo-root to specify)");
            std::process::exit(1);
        }
        info!(
            "config loaded: {} repo(s) in {:.0?}",
            roots.len(),
            startup.elapsed()
        );
        roots
    } else {
        if !cli.repo_root.is_empty() {
            eprintln!(
                "Warning: --repo-root is ignored in socket mode (repos are managed by the daemon)"
            );
        }
        vec![]
    };

    // Spawn daemon init on a separate task so it runs concurrently with the splash
    // (show_splash uses blocking crossterm::event::poll calls).
    let config_clone = Arc::clone(&config);
    let daemon_task = tokio::spawn(async move {
        let daemon: Arc<dyn DaemonHandle> = if embedded {
            let d = InProcessDaemon::new(repo_roots, config_clone).await;
            d as Arc<dyn DaemonHandle>
        } else {
            let socket_path = cli.socket_path();
            flotilla_tui::socket::connect_or_spawn(
                &socket_path,
                &cli.config_dir(),
                cli.config_dir.as_deref(),
                cli.socket.as_deref(),
            )
            .await
            .map_err(|e| color_eyre::eyre::eyre!(e))
            .expect("failed to connect to daemon")
        };
        info!("daemon ready in {:.0?}", startup.elapsed());
        daemon
    });

    flotilla_tui::splash::show_splash(&mut terminal).await?;
    let daemon = daemon_task.await.expect("daemon init panicked");

    let daemon_rx = daemon.subscribe();
    let repos_info = daemon.list_repos().await.unwrap_or_default();
    let mut app = app::App::new(daemon.clone(), repos_info, Arc::clone(&config));

    // Get initial state via replay_since (works for both in-process and socket).
    let replay_events = daemon
        .replay_since(&std::collections::HashMap::new())
        .await
        .unwrap_or_default();
    for event in replay_events {
        app.handle_daemon_event(event);
    }

    execute!(stdout(), EnableMouseCapture)?;
    let mut events = event::EventHandler::new(Duration::from_millis(250));
    events.attach_daemon(daemon_rx);

    loop {
        terminal.draw(|f| ui::render(&app.model, &mut app.ui, &app.in_flight, f))?;

        if let Some(evt) = events.next().await {
            match evt {
                event::Event::Daemon(daemon_evt) => {
                    app.handle_daemon_event(daemon_evt);
                }
                event::Event::Key(k) => {
                    let is_normal = matches!(app.ui.mode, app::UiMode::Normal);
                    if k.code == crossterm::event::KeyCode::Char('r') && is_normal {
                        // Trigger immediate refresh on active repo via daemon
                        let repo = app.model.active_repo_root().clone();
                        let daemon = app.daemon.clone();
                        tokio::spawn(async move {
                            let _ = daemon.refresh(&repo).await;
                        });
                    } else {
                        app.handle_key(k);
                    }
                }
                event::Event::Mouse(m) => {
                    use crossterm::event::{MouseButton, MouseEventKind};
                    match m.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let x = m.column;
                            let y = m.row;
                            let mut tab_clicked = false;

                            // Check event log filter area (click cycles level)
                            let ef = app.ui.layout.event_log_filter_area;
                            if x >= ef.x && x < ef.x + ef.width && y >= ef.y && y < ef.y + ef.height
                            {
                                app.ui.event_log.filter = app.ui.event_log.filter.cycle();
                                app.ui.event_log.count = 0;
                                tab_clicked = true;
                            }

                            // Check which tab area was clicked
                            if !tab_clicked {
                                let hit = app
                                    .ui
                                    .layout
                                    .tab_areas
                                    .iter()
                                    .find(|(_, r)| {
                                        x >= r.x
                                            && x < r.x + r.width
                                            && y >= r.y
                                            && y < r.y + r.height
                                    })
                                    .map(|(id, _)| id.clone());

                                match hit {
                                    Some(app::TabId::Flotilla) => {
                                        app.ui.mode = app::UiMode::Config;
                                        app.ui.drag.dragging_tab = None;
                                        tab_clicked = true;
                                    }
                                    Some(app::TabId::Repo(i)) => {
                                        app.switch_tab(i);
                                        app.ui.drag.dragging_tab = Some(i);
                                        app.ui.drag.start_x = x;
                                        app.ui.drag.active = false;
                                        tab_clicked = true;
                                    }
                                    Some(app::TabId::Gear) if !app.ui.mode.is_config() => {
                                        let sp = app.active_ui().show_providers;
                                        app.active_ui_mut().show_providers = !sp;
                                        tab_clicked = true;
                                    }
                                    Some(app::TabId::Add) => {
                                        let mut input = tui_input::Input::default();
                                        if let Some(parent) = app.model.active_repo_root().parent()
                                        {
                                            let parent_str = format!("{}/", parent.display());
                                            input = tui_input::Input::from(parent_str.as_str());
                                        }
                                        app.ui.mode = app::UiMode::FilePicker {
                                            input,
                                            dir_entries: Vec::new(),
                                            selected: 0,
                                        };
                                        app.refresh_dir_listing();
                                        tab_clicked = true;
                                    }
                                    _ => {}
                                }
                            }
                            if !tab_clicked {
                                app.ui.drag.dragging_tab = None;
                                app.handle_mouse(m);
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(dragging_idx) = app.ui.drag.dragging_tab {
                                if !app.ui.drag.active {
                                    let dx = (m.column as i16 - app.ui.drag.start_x as i16)
                                        .unsigned_abs();
                                    if dx >= 2 {
                                        app.ui.drag.active = true;
                                    }
                                }
                                if app.ui.drag.active {
                                    for (id, r) in &app.ui.layout.tab_areas {
                                        if let app::TabId::Repo(i) = *id {
                                            if m.column >= r.x
                                                && m.column < r.x + r.width
                                                && m.row >= r.y
                                                && m.row < r.y + r.height
                                                && i != dragging_idx
                                            {
                                                app.model.repo_order.swap(dragging_idx, i);
                                                app.model.active_repo = i;
                                                app.ui.drag.dragging_tab = Some(i);
                                                break;
                                            }
                                        }
                                    }
                                }
                            } else {
                                app.handle_mouse(m);
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            if app.ui.drag.dragging_tab.take().is_some() {
                                if app.ui.drag.active {
                                    app.config.save_tab_order(&app.model.repo_order);
                                }
                                app.ui.drag.active = false;
                            }
                        }
                        _ => {
                            app.handle_mouse(m);
                        }
                    }
                }
                event::Event::Tick => {}
            }
        }

        // Process proto command queue — routed through daemon-side executor
        while let Some(cmd) = app.proto_commands.take_next() {
            app::executor::dispatch(cmd, &mut app).await;
        }

        if app.should_quit {
            break;
        }
    }

    execute!(stdout(), DisableMouseCapture)?;
    ratatui::restore();
    Ok(())
}

async fn run_daemon(cli: &Cli, timeout_secs: u64) -> Result<()> {
    // Initialize logging to stderr (no TUI here)
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::DEBUG.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();

    let socket_path = cli.socket_path();
    let timeout = if timeout_secs == 0 {
        Duration::from_secs(u64::MAX)
    } else {
        Duration::from_secs(timeout_secs)
    };

    // Load repos from config
    let config = Arc::new(ConfigStore::new());
    let repo_roots = config.load_repos();
    info!("starting daemon with {} repo(s)", repo_roots.len());

    let server =
        flotilla_daemon::server::DaemonServer::new(repo_roots, config, socket_path, timeout).await;

    server.run().await.map_err(|e| color_eyre::eyre::eyre!(e))
}

async fn run_status(cli: &Cli) -> Result<()> {
    let socket_path = cli.socket_path();
    let daemon = SocketDaemon::connect(&socket_path)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("cannot connect to daemon: {e}"))?;

    let repos = daemon
        .list_repos()
        .await
        .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;

    if repos.is_empty() {
        println!("No repos tracked.");
        return Ok(());
    }

    for repo in &repos {
        let name = &repo.name;
        let path = repo.path.display();
        let health: Vec<String> = repo
            .provider_health
            .iter()
            .map(|(k, v)| format!("{k}: {}", if *v { "ok" } else { "error" }))
            .collect();
        let loading = if repo.loading { " (loading)" } else { "" };
        println!("{name}{loading}  {path}");
        if !health.is_empty() {
            println!("  providers: {}", health.join(", "));
        }
    }

    Ok(())
}

async fn run_watch(cli: &Cli) -> Result<()> {
    let socket_path = cli.socket_path();
    let daemon = SocketDaemon::connect(&socket_path)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("cannot connect to daemon: {e}"))?;

    let mut rx = daemon.subscribe();
    println!("watching events (Ctrl-C to stop)...");

    loop {
        match rx.recv().await {
            Ok(event) => {
                let json =
                    serde_json::to_string_pretty(&event).unwrap_or_else(|_| format!("{event:?}"));
                println!("{json}");
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("warning: skipped {n} events");
            }
            Err(_) => {
                eprintln!("daemon disconnected");
                break;
            }
        }
    }

    Ok(())
}
