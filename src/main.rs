mod app;
mod data;
mod event;
mod event_log;
mod template;
mod ui;
mod config;
mod providers;

use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;
use clap::Parser;
use color_eyre::Result;
use crossterm::{execute, event::{EnableMouseCapture, DisableMouseCapture}};
/// Flotilla: TUI dashboard for managing development workspaces across cmux, git worktrees, and GitHub.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Git repo roots (repeatable; auto-detected from cwd if omitted)
    #[arg(long)]
    repo_root: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    event_log::init();
    let cli = Cli::parse();

    let startup = std::time::Instant::now();

    // Collect repos: persisted (in saved order) first, then CLI args, then auto-detect
    let mut repo_roots: Vec<PathBuf> = Vec::new();

    // 1. Persisted repos in saved tab order
    let persisted = config::load_repos();
    let tab_order = config::load_tab_order();
    if let Some(order) = tab_order {
        for path in &order {
            if persisted.contains(path) && !repo_roots.contains(path) {
                repo_roots.push(path.clone());
            }
        }
        // Any persisted repos not in the order file go at the end
        for path in &persisted {
            if !repo_roots.contains(path) {
                repo_roots.push(path.clone());
            }
        }
    } else {
        repo_roots.extend(persisted);
    }

    // 2. CLI args (appended after persisted)
    for root in &cli.repo_root {
        let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if !repo_roots.contains(&canonical) {
            repo_roots.push(canonical);
        }
    }

    // 3. Auto-detect from cwd if nothing found yet
    if repo_roots.is_empty() {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
                repo_roots.push(path);
            }
        }
    }

    // Persist any new repos and save tab order
    for path in &repo_roots {
        config::save_repo(path);
    }
    config::save_tab_order(&repo_roots);

    if repo_roots.is_empty() {
        eprintln!("Error: no git repositories found (use --repo-root to specify)");
        std::process::exit(1);
    }

    info!("config loaded: {} repo(s) in {:.0?}", repo_roots.len(), startup.elapsed());

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    show_splash(&mut terminal)?;
    let result = run(&mut terminal, repo_roots).await;
    execute!(stdout(), DisableMouseCapture)?;
    ratatui::restore();
    result
}

async fn run(terminal: &mut ratatui::DefaultTerminal, repo_roots: Vec<PathBuf>) -> Result<()> {
    let t = std::time::Instant::now();
    let mut app = app::App::new(repo_roots);
    info!("provider detection in {:.0?}", t.elapsed());

    // Mark all repos as loading so ⟳ shows on first render
    for rs in app.repos.values_mut() {
        rs.data.loading = true;
    }

    let mut events = event::EventHandler::new(Duration::from_millis(250));
    // Set last_refresh to the past so the first tick triggers an immediate refresh
    let mut last_refresh = std::time::Instant::now() - Duration::from_secs(60);
    let refresh_interval = Duration::from_secs(10);

    loop {
        terminal.draw(|f| ui::render(&mut app, f))?;

        if let Some(evt) = events.next().await {
            match evt {
                event::Event::Key(k) => {
                    if k.code == crossterm::event::KeyCode::Char('r')
                        && !app.show_action_menu
                        && app.input_mode == app::InputMode::Normal
                        && !app.show_help
                        && !app.show_delete_confirm
                    {
                        refresh_all(&mut app).await;
                        last_refresh = std::time::Instant::now();
                    } else {
                        app.handle_key(k);
                    }
                }
                event::Event::Mouse(m) => {
                    use crossterm::event::{MouseEventKind, MouseButton};
                    match m.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let x = m.column;
                            let y = m.row;
                            let mut tab_clicked = false;

                            // Check event log filter area (click cycles level)
                            let ef = app.event_log_filter_area;
                            if x >= ef.x && x < ef.x + ef.width && y >= ef.y && y < ef.y + ef.height {
                                app.event_log_filter = app.event_log_filter.cycle();
                                // Reset scroll to bottom on filter change
                                app.event_log_count = 0;
                                tab_clicked = true;
                            }

                            // Check flotilla pseudo-tab area
                            if !tab_clicked {
                                let fa = app.flotilla_tab_area;
                                if x >= fa.x && x < fa.x + fa.width && y >= fa.y && y < fa.y + fa.height {
                                    app.show_config = true;
                                    app.dragging_tab = None;
                                    tab_clicked = true;
                                }
                            }

                            // Check repo tab areas
                            if !tab_clicked {
                                for (i, tab_area) in app.tab_areas.iter().enumerate() {
                                    if x >= tab_area.x && x < tab_area.x + tab_area.width
                                        && y >= tab_area.y && y < tab_area.y + tab_area.height
                                    {
                                        app.switch_tab(i);
                                        app.dragging_tab = Some(i);
                                        app.drag_start_x = x;
                                        app.drag_active = false;
                                        tab_clicked = true;
                                        break;
                                    }
                                }
                            }
                            // Check gear icon area (per-repo provider toggle)
                            if !tab_clicked && !app.show_config {
                                let ga = app.gear_icon_area;
                                if x >= ga.x && x < ga.x + ga.width && y >= ga.y && y < ga.y + ga.height {
                                    let sp = app.active().show_providers;
                                    app.active_mut().show_providers = !sp;
                                    tab_clicked = true;
                                }
                            }
                            if !tab_clicked {
                                app.dragging_tab = None;
                                // Check [+] button
                                let a = app.add_tab_area;
                                if x >= a.x && x < a.x + a.width && y >= a.y && y < a.y + a.height {
                                    app.input_mode = app::InputMode::AddRepo;
                                    app.input.reset();
                                    if let Some(parent) = app.active_repo_root().parent() {
                                        let parent_str = format!("{}/", parent.display());
                                        app.input = tui_input::Input::from(parent_str.as_str());
                                    }
                                    app.dir_entries = Vec::new();
                                    app.dir_selected = 0;
                                    app.refresh_dir_listing();
                                } else {
                                    app.handle_mouse(m);
                                }
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(dragging_idx) = app.dragging_tab {
                                if !app.drag_active {
                                    let dx = (m.column as i16 - app.drag_start_x as i16).unsigned_abs();
                                    if dx >= 2 {
                                        app.drag_active = true;
                                    }
                                }
                                if app.drag_active {
                                    for (i, tab_area) in app.tab_areas.iter().enumerate() {
                                        if m.column >= tab_area.x
                                            && m.column < tab_area.x + tab_area.width
                                            && m.row >= tab_area.y
                                            && m.row < tab_area.y + tab_area.height
                                            && i != dragging_idx
                                        {
                                            app.repo_order.swap(dragging_idx, i);
                                            app.active_repo = i;
                                            app.dragging_tab = Some(i);
                                            break;
                                        }
                                    }
                                }
                            } else {
                                app.handle_mouse(m);
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            if app.dragging_tab.take().is_some() {
                                if app.drag_active {
                                    config::save_tab_order(&app.repo_order);
                                }
                                app.drag_active = false;
                            }
                        }
                        _ => {
                            app.handle_mouse(m);
                        }
                    }
                }
                event::Event::Tick => {
                    if last_refresh.elapsed() >= refresh_interval {
                        refresh_all(&mut app).await;
                        last_refresh = std::time::Instant::now();
                    }
                }
            }
        }

        // Process pending actions — clear status only when user triggers an action
        let pending = app.take_pending_action();
        if !matches!(pending, app::PendingAction::None) {
            app.status_message = None;
        }
        match pending {
            app::PendingAction::SwitchWorktree(i) => {
                if let Some(co) = app.active().data.checkouts.get(i).cloned() {
                    info!("entering workspace for {}", co.branch);
                    let ws_result = if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
                        let config = workspace_config(app.active_repo_root(), &co.branch, &co.path, "claude");
                        Some(ws_mgr.create_workspace(&config).await)
                    } else {
                        None
                    };
                    if let Some(Err(e)) = ws_result {
                        app.status_message = Some(e);
                    }
                    refresh_all(&mut app).await;
                }
            }
            app::PendingAction::SelectWorkspace(ws_ref) => {
                info!("switching to workspace {ws_ref}");
                if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
                    if let Err(e) = ws_mgr.select_workspace(&ws_ref).await {
                        app.status_message = Some(e);
                    }
                }
            }
            app::PendingAction::FetchDeleteInfo(si) => {
                let table_idx = app.active().data.selectable_indices.get(si).copied();
                if let Some(table_idx) = table_idx {
                    if let Some(data::TableEntry::Item(item)) = app.active().data.table_entries.get(table_idx).cloned() {
                        let branch = item.branch.clone().unwrap_or_default();
                        let wt_path = item.worktree_idx
                            .and_then(|idx| app.active().data.checkouts.get(idx))
                            .map(|co| co.path.clone());
                        let pr_id = item.pr_idx
                            .and_then(|idx| app.active().data.change_requests.get(idx))
                            .map(|cr| cr.id.clone());
                        let repo_root = app.active_repo_root().clone();
                        let info = data::fetch_delete_confirm_info(
                            &branch,
                            wt_path.as_deref(),
                            pr_id.as_deref(),
                            &repo_root,
                        ).await;
                        app.delete_confirm_info = Some(info);
                        app.delete_confirm_loading = false;
                    }
                }
            }
            app::PendingAction::ConfirmDelete => {
                if let Some(info) = app.delete_confirm_info.take() {
                    info!("deleting worktree {}", info.branch);
                    let repo = app.active_repo_root().clone();
                    let result = if let Some(cm) = app.active().registry.checkout_managers.values().next() {
                        Some(cm.remove_checkout(repo.as_path(), &info.branch).await)
                    } else {
                        None
                    };
                    if let Some(Err(e)) = result {
                        app.status_message = Some(e);
                    }
                    refresh_all(&mut app).await;
                }
            }
            app::PendingAction::OpenPr(id) => {
                debug!("opening PR {id} in browser");
                let repo = app.active_repo_root().clone();
                if let Some(cr) = app.active().registry.code_review.values().next() {
                    let _ = cr.open_in_browser(&repo, &id).await;
                }
            }
            app::PendingAction::OpenIssueBrowser(id) => {
                debug!("opening issue {id} in browser");
                let repo = app.active_repo_root().clone();
                if let Some(it) = app.active().registry.issue_trackers.values().next() {
                    let _ = it.open_in_browser(&repo, &id).await;
                }
            }
            app::PendingAction::CreateWorktree(branch) => {
                info!("creating worktree {branch}");
                let repo = app.active_repo_root().clone();
                let checkout_result = if let Some(cm) = app.active().registry.checkout_managers.values().next() {
                    Some(cm.create_checkout(repo.as_path(), &branch).await)
                } else {
                    None
                };
                match checkout_result {
                    Some(Ok(checkout)) => {
                        info!("created worktree at {}", checkout.path.display());
                        let ws_result = if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
                            let config = workspace_config(app.active_repo_root(), &branch, &checkout.path, "claude");
                            Some(ws_mgr.create_workspace(&config).await)
                        } else {
                            None
                        };
                        if let Some(Err(e)) = ws_result {
                            app.status_message = Some(e);
                        }
                    }
                    Some(Err(e)) => app.status_message = Some(e),
                    None => app.status_message = Some("No checkout manager available".to_string()),
                }
                refresh_all(&mut app).await;
            }
            app::PendingAction::ArchiveSession(ses_idx) => {
                if let Some(session) = app.active().data.sessions.get(ses_idx).cloned() {
                    info!("archiving session {}", session.id);
                    let result = if let Some(ca) = app.active().registry.coding_agents.values().next() {
                        Some(ca.archive_session(&session.id).await)
                    } else {
                        None
                    };
                    if let Some(Err(e)) = result {
                        app.status_message = Some(e);
                    }
                    refresh_all(&mut app).await;
                }
            }
            app::PendingAction::TeleportSession { session_id, branch, worktree_idx } => {
                info!("teleporting to session {session_id}");
                let teleport_cmd = format!("claude --teleport {}", session_id);
                let wt_path = if let Some(wt_idx) = worktree_idx {
                    app.active().data.checkouts.get(wt_idx).map(|co| co.path.clone())
                } else if let Some(branch_name) = &branch {
                    let repo = app.active_repo_root().clone();
                    let checkout_result = if let Some(cm) = app.active().registry.checkout_managers.values().next() {
                        cm.create_checkout(repo.as_path(), branch_name).await.ok()
                    } else {
                        None
                    };
                    checkout_result.map(|c| c.path)
                } else {
                    None
                };
                if let Some(path) = wt_path {
                    let name = branch.as_deref().unwrap_or("session");
                    let ws_result = if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
                        let config = workspace_config(app.active_repo_root(), name, &path, &teleport_cmd);
                        Some(ws_mgr.create_workspace(&config).await)
                    } else {
                        None
                    };
                    if let Some(Err(e)) = ws_result {
                        app.status_message = Some(e);
                    }
                }
                refresh_all(&mut app).await;
            }
            app::PendingAction::GenerateBranchName(issue_idxs) => {
                let issues: Vec<(String, String)> = issue_idxs
                    .iter()
                    .filter_map(|&idx| app.active().data.issues.get(idx))
                    .map(|issue| (issue.id.clone(), issue.title.clone()))
                    .collect();

                info!("generating branch name");
                let branch_result = if let Some(ai) = app.active().registry.ai_utilities.values().next() {
                    let context: Vec<String> = issues.iter()
                        .map(|(id, title)| format!("{} #{}", title, id))
                        .collect();
                    let prompt_text = if context.len() == 1 {
                        context[0].clone()
                    } else {
                        context.join("; ")
                    };
                    Some(ai.generate_branch_name(&prompt_text).await)
                } else {
                    None
                };
                match branch_result {
                    Some(Ok(branch)) => {
                        info!("AI suggested: {branch}");
                        app.prefill_branch_input(&branch);
                    }
                    // None = no AI provider, Some(Err(_)) = AI call failed
                    _ => {
                        let fallback: Vec<String> = issues.iter()
                            .map(|(id, _)| format!("issue-{}", id))
                            .collect();
                        app.prefill_branch_input(&fallback.join("-"));
                    }
                }
            }
            app::PendingAction::AddRepo(path) => {
                info!("adding repo {}", path.display());
                config::save_repo(&path);
                app.add_repo(path);
                app.switch_tab(app.repo_order.len() - 1);
                config::save_tab_order(&app.repo_order);
                refresh_all(&mut app).await;
            }
            app::PendingAction::None => {}
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn workspace_config(
    repo_root: &std::path::Path,
    name: &str,
    working_dir: &std::path::Path,
    main_command: &str,
) -> crate::providers::types::WorkspaceConfig {
    let tmpl_path = repo_root.join(".flotilla/workspace.yaml");
    let template_yaml = std::fs::read_to_string(&tmpl_path).ok();
    let mut template_vars = std::collections::HashMap::new();
    template_vars.insert("main_command".to_string(), main_command.to_string());
    crate::providers::types::WorkspaceConfig {
        name: name.to_string(),
        working_directory: working_dir.to_path_buf(),
        template_vars,
        template_yaml,
    }
}

fn show_splash(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    use ratatui_image::{picker::Picker, StatefulImage};

    let img_bytes = include_bytes!("../assets/splash.png");
    let dyn_img = image::load_from_memory(img_bytes)
        .map_err(|e| color_eyre::eyre::eyre!("splash image: {e}"))?;

    let img_w = dyn_img.width() as f64;
    let img_h = dyn_img.height() as f64;

    let picker = Picker::from_query_stdio()
        .unwrap_or_else(|_| Picker::halfblocks());
    let mut protocol = picker.new_resize_protocol(dyn_img);

    let deadline = std::time::Instant::now() + Duration::from_millis(700);

    loop {
        terminal.draw(|f| {
            use ratatui::layout::{Constraint, Flex, Layout};
            let area = f.area();
            // Halfblocks: 1 col ≈ 1px wide, 1 row ≈ 2px tall
            let scale = (area.width as f64 / img_w).min(area.height as f64 * 2.0 / img_h);
            let rw = (img_w * scale) as u16;
            let rh = (img_h * scale / 2.0) as u16;
            let [area] = Layout::horizontal([Constraint::Length(rw.min(area.width))])
                .flex(Flex::Center)
                .areas(area);
            let [area] = Layout::vertical([Constraint::Length(rh.min(area.height))])
                .flex(Flex::Center)
                .areas(area);
            let widget = StatefulImage::default();
            f.render_stateful_widget(widget, area, &mut protocol);
        })?;

        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if crossterm::event::poll(remaining.min(Duration::from_millis(50)))? {
            let _ = crossterm::event::read()?;
            break;
        }
    }
    Ok(())
}

use tracing::{info, debug, error};
use event_log::LevelExt;

async fn refresh_all(app: &mut app::App) {
    let t = std::time::Instant::now();
    // Snapshot all repos for change detection
    let snapshots: Vec<_> = app.repo_order.iter()
        .map(|path| app.repos[path].data_snapshot())
        .collect();

    // Extract data stores AND registries (both moved out)
    let items: Vec<(PathBuf, data::DataStore, providers::registry::ProviderRegistry, providers::types::RepoCriteria)> = app.repo_order.iter()
        .map(|path| {
            let rs = app.repos.get_mut(path).unwrap();
            let ds = std::mem::take(&mut rs.data);
            let reg = std::mem::take(&mut rs.registry);
            let criteria = rs.repo_criteria.clone();
            (path.clone(), ds, reg, criteria)
        })
        .collect();

    let results = futures::future::join_all(
        items.into_iter().map(|(root, mut ds, registry, criteria)| {
            async move {
                let errors = ds.refresh(&root, &registry, &criteria).await;
                (root, ds, registry, errors)
            }
        })
    ).await;

    let mut all_errors: Vec<String> = Vec::new();
    for (i, (path, data, registry, errors)) in results.into_iter().enumerate() {
        let rs = app.repos.get_mut(&path).unwrap();
        rs.data = data;
        rs.registry = registry;

        // Change detection
        let new_snapshot = rs.data_snapshot();
        if snapshots[i] != new_snapshot && i != app.active_repo {
            rs.has_unseen_changes = true;
        }

        // Restore selection
        if rs.data.selectable_indices.is_empty() {
            rs.selected_selectable_idx = None;
            rs.table_state.select(None);
        } else if rs.selected_selectable_idx.is_none() {
            rs.selected_selectable_idx = Some(0);
            rs.table_state.select(Some(rs.data.selectable_indices[0]));
        } else if let Some(si) = rs.selected_selectable_idx {
            let clamped = si.min(rs.data.selectable_indices.len() - 1);
            rs.selected_selectable_idx = Some(clamped);
            rs.table_state.select(Some(rs.data.selectable_indices[clamped]));
        }

        // Track per-provider statuses and log errors
        let name = app::App::repo_name(&path);

        // Mark coding agents as ok/error based on session fetch result
        for (pname, _) in rs.registry.coding_agents.iter() {
            let key = (path.clone(), "coding_agent".into(), pname.clone());
            if errors.iter().any(|e| e.contains("session") || e.contains("Session") || e.contains("auth") || e.contains("credential")) {
                app.provider_statuses.insert(key, app::ProviderStatus::Error);
            } else {
                app.provider_statuses.insert(key, app::ProviderStatus::Ok);
            }
        }

        // Mark code review / issue tracker based on errors
        for (pname, _) in rs.registry.code_review.iter() {
            let key = (path.clone(), "code_review".into(), pname.clone());
            if errors.iter().any(|e| e.contains("PR") || e.contains("pull")) {
                app.provider_statuses.insert(key, app::ProviderStatus::Error);
            } else {
                app.provider_statuses.insert(key, app::ProviderStatus::Ok);
            }
        }
        for (pname, _) in rs.registry.issue_trackers.iter() {
            let key = (path.clone(), "issue_tracker".into(), pname.clone());
            if errors.iter().any(|e| e.contains("issue") || e.contains("Issue")) {
                app.provider_statuses.insert(key, app::ProviderStatus::Error);
            } else {
                app.provider_statuses.insert(key, app::ProviderStatus::Ok);
            }
        }

        if !errors.is_empty() {
            for e in &errors {
                error!("{name}: {e}");
                all_errors.push(format!("{name}: {e}"));
            }
        }
    }

    debug!("refresh complete in {:.0?}", t.elapsed());

    if !all_errors.is_empty() {
        app.status_message = Some(all_errors.join("; "));
    }
}
