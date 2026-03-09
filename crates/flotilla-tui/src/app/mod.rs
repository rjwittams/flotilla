pub mod executor;
pub mod intent;
mod key_handlers;
mod navigation;
pub mod ui_state;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use tui_input::backend::crossterm::EventHandler as InputEventHandler;
use tui_input::Input;

use flotilla_core::config::ConfigStore;
use flotilla_core::daemon::DaemonHandle;
use flotilla_core::data::{self, GroupEntry, SectionLabels};
use flotilla_protocol::{
    Command, DaemonEvent, ProviderData, ProviderError, RepoInfo, RepoLabels, Snapshot,
    SnapshotDelta, WorkItem,
};
use std::collections::VecDeque;

pub use intent::Intent;
pub use ui_state::{DirEntry, RepoUiState, TabId, UiMode, UiState};

/// Per-provider auth/health status from last refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderStatus {
    Ok,
    Error,
}

#[derive(Default)]
pub struct CommandQueue {
    queue: VecDeque<Command>,
}

impl CommandQueue {
    pub fn push(&mut self, cmd: Command) {
        self.queue.push_back(cmd);
    }
    pub fn take_next(&mut self) -> Option<Command> {
        self.queue.pop_front()
    }
}

/// Per-repo view-model state for the TUI. Contains only what the UI needs
/// to render — no provider registry, no refresh handle.
pub struct TuiRepoModel {
    pub providers: Arc<ProviderData>,
    pub labels: RepoLabels,
    pub provider_names: HashMap<String, String>,
    pub provider_health: HashMap<String, bool>,
    pub loading: bool,
    pub issue_has_more: bool,
    pub issue_total: Option<u32>,
    pub issue_search_active: bool,
    pub issue_fetch_pending: bool,
    /// Whether the initial issue fetch has been requested for this repo.
    pub issue_initial_requested: bool,
}

/// TUI-side domain model. Mirrors the shape of core's `AppModel` but without
/// daemon-internal fields (registry, refresh handles). Populated from
/// `DaemonHandle::list_repos()` and updated by daemon snapshot events.
pub struct TuiModel {
    pub repos: HashMap<PathBuf, TuiRepoModel>,
    pub repo_order: Vec<PathBuf>,
    pub active_repo: usize,
    /// Per-repo, per-provider auth status from last refresh.
    /// Key: (repo_path, provider_category, provider_name)
    pub provider_statuses: HashMap<(PathBuf, String, String), ProviderStatus>,
    pub status_message: Option<String>,
}

impl TuiModel {
    pub fn from_repo_info(repos_info: Vec<RepoInfo>) -> Self {
        let mut repos = HashMap::new();
        let mut order = Vec::new();
        for info in repos_info {
            repos.insert(
                info.path.clone(),
                TuiRepoModel {
                    providers: Arc::new(ProviderData::default()),
                    labels: info.labels,
                    provider_names: info.provider_names,
                    provider_health: info.provider_health,
                    loading: info.loading,
                    issue_has_more: false,
                    issue_total: None,
                    issue_search_active: false,
                    issue_fetch_pending: false,
                    issue_initial_requested: false,
                },
            );
            order.push(info.path);
        }
        Self {
            repos,
            repo_order: order,
            active_repo: 0,
            provider_statuses: HashMap::new(),
            status_message: None,
        }
    }

    pub fn active(&self) -> &TuiRepoModel {
        &self.repos[&self.repo_order[self.active_repo]]
    }

    pub fn active_repo_root(&self) -> &PathBuf {
        &self.repo_order[self.active_repo]
    }

    pub fn active_labels(&self) -> &RepoLabels {
        &self.active().labels
    }

    pub fn repo_name(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string())
    }
}

/// A command that has been dispatched to the daemon and is awaiting completion.
pub struct InFlightCommand {
    pub repo: PathBuf,
    pub description: String,
}

/// Log provider errors and format them into a status message.
///
/// Suppresses "issues disabled" messages since the daemon handles those.
/// Returns `None` when there are no displayable errors.
fn format_error_status(errors: &[ProviderError], repo_path: &Path) -> Option<String> {
    let name = TuiModel::repo_name(repo_path);
    let mut all_errors: Vec<String> = Vec::new();
    for e in errors {
        if e.category == "issues" && e.message.contains("has disabled issues") {
            continue;
        }
        tracing::error!("{name}: {}: {}", e.category, e.message);
        all_errors.push(format!("{name}: {}: {}", e.category, e.message));
    }
    if all_errors.is_empty() {
        None
    } else {
        Some(all_errors.join("; "))
    }
}

pub struct App {
    pub daemon: Arc<dyn DaemonHandle>,
    pub config: Arc<ConfigStore>,
    pub model: TuiModel,
    pub ui: UiState,
    pub proto_commands: CommandQueue,
    pub in_flight: HashMap<u64, InFlightCommand>,
    pub should_quit: bool,
}

impl App {
    pub fn new(
        daemon: Arc<dyn DaemonHandle>,
        repos_info: Vec<RepoInfo>,
        config: Arc<ConfigStore>,
    ) -> Self {
        let model = TuiModel::from_repo_info(repos_info);
        let ui = UiState::new(&model.repo_order);
        Self {
            daemon,
            config,
            model,
            ui,
            proto_commands: Default::default(),
            in_flight: HashMap::new(),
            should_quit: false,
        }
    }

    // ── Daemon event handling ──

    pub fn handle_daemon_event(&mut self, event: DaemonEvent) {
        match event {
            DaemonEvent::SnapshotFull(snap) => self.apply_snapshot(*snap),
            DaemonEvent::SnapshotDelta(delta) => self.apply_delta(*delta),
            DaemonEvent::RepoAdded(info) => self.handle_repo_added(*info),
            DaemonEvent::RepoRemoved { path } => self.handle_repo_removed(&path),
            DaemonEvent::CommandStarted {
                command_id,
                repo,
                description,
            } => {
                tracing::info!("command {command_id} started: {description}");
                self.in_flight
                    .insert(command_id, InFlightCommand { repo, description });
            }
            DaemonEvent::CommandFinished {
                command_id, result, ..
            } => {
                if let Some(_cmd) = self.in_flight.remove(&command_id) {
                    tracing::info!("command {command_id} finished");
                    executor::handle_result(result, self);
                }
            }
        }
    }

    fn apply_snapshot(&mut self, snap: Snapshot) {
        let path = snap.repo.clone();
        let rm = match self.model.repos.get_mut(&path) {
            Some(rm) => rm,
            None => return,
        };

        let old_providers = std::mem::replace(&mut rm.providers, Arc::new(snap.providers));
        rm.provider_health = snap.provider_health.clone();
        rm.loading = false;
        rm.issue_has_more = snap.issue_has_more;
        rm.issue_total = snap.issue_total;
        rm.issue_search_active = snap.issue_search_results.is_some();
        rm.issue_fetch_pending = false;

        // Build table view
        let section_labels = SectionLabels {
            checkouts: rm.labels.checkouts.section.clone(),
            code_review: rm.labels.code_review.section.clone(),
            issues: rm.labels.issues.section.clone(),
            sessions: rm.labels.sessions.section.clone(),
        };
        let table_view = data::group_work_items(&snap.work_items, &rm.providers, &section_labels);

        // Provider health -> model-level statuses
        for (kind, healthy) in &rm.provider_health {
            let provider_name = rm.provider_names.get(kind.as_str()).cloned();
            if let Some(pname) = provider_name {
                let key = (path.clone(), kind.clone(), pname);
                let status = if *healthy {
                    ProviderStatus::Ok
                } else {
                    ProviderStatus::Error
                };
                self.model.provider_statuses.insert(key, status);
            }
        }

        // Change detection badge for inactive tabs
        let active_idx = self.model.active_repo;
        let i = self.model.repo_order.iter().position(|p| p == &path);
        if let Some(idx) = i {
            if idx != active_idx && *old_providers != *rm.providers {
                if let Some(rui) = self.ui.repo_ui.get_mut(&path) {
                    rui.has_unseen_changes = true;
                }
            }
        }

        if let Some(rui) = self.ui.repo_ui.get_mut(&path) {
            rui.update_table_view(table_view);
        }

        // Log and display errors (clears status when errors resolve)
        self.model.status_message = format_error_status(&snap.errors, &path);

        // Request initial issue fetch once per repo (on first snapshot received)
        let rm = self.model.repos.get_mut(&path).unwrap();
        if !rm.issue_initial_requested {
            rm.issue_initial_requested = true;
            let visible = self.ui.layout.table_area.height.saturating_sub(2) as usize;
            self.proto_commands.push(Command::SetIssueViewport {
                repo: path,
                visible_count: visible.max(20),
            });
        }
    }

    fn apply_delta(&mut self, delta: SnapshotDelta) {
        let path = delta.repo;
        let rm = match self.model.repos.get_mut(&path) {
            Some(rm) => rm,
            None => return,
        };

        // Apply provider data changes
        let mut providers = (*rm.providers).clone();
        flotilla_core::delta::apply_changes(&mut providers, delta.changes.clone());
        rm.providers = Arc::new(providers);

        // Update issue metadata
        rm.issue_has_more = delta.issue_has_more;
        rm.issue_total = delta.issue_total;
        rm.issue_search_active = delta.issue_search_results.is_some();
        rm.issue_fetch_pending = false;

        // Apply provider health and error changes from the delta
        for change in &delta.changes {
            match change {
                flotilla_protocol::Change::ProviderHealth {
                    provider,
                    op:
                        flotilla_protocol::EntryOp::Added(v) | flotilla_protocol::EntryOp::Updated(v),
                } => {
                    rm.provider_health.insert(provider.clone(), *v);
                }
                flotilla_protocol::Change::ProviderHealth {
                    provider,
                    op: flotilla_protocol::EntryOp::Removed,
                } => {
                    rm.provider_health.remove(provider);
                }
                flotilla_protocol::Change::ErrorsChanged(errors) => {
                    self.model.status_message = format_error_status(errors, &path);
                }
                _ => {}
            }
        }

        // Re-correlate and rebuild table view
        let (work_items, correlation_groups) = data::correlate(&rm.providers);
        let proto_work_items: Vec<WorkItem> = work_items
            .iter()
            .map(|wi| {
                flotilla_core::convert::correlation_result_to_work_item(wi, &correlation_groups)
            })
            .collect();

        let section_labels = SectionLabels {
            checkouts: rm.labels.checkouts.section.clone(),
            code_review: rm.labels.code_review.section.clone(),
            issues: rm.labels.issues.section.clone(),
            sessions: rm.labels.sessions.section.clone(),
        };
        let table_view = data::group_work_items(&proto_work_items, &rm.providers, &section_labels);

        // Provider health -> model-level statuses
        for (kind, healthy) in &rm.provider_health {
            let provider_name = rm.provider_names.get(kind.as_str()).cloned();
            if let Some(pname) = provider_name {
                let key = (path.clone(), kind.clone(), pname);
                let status = if *healthy {
                    ProviderStatus::Ok
                } else {
                    ProviderStatus::Error
                };
                self.model.provider_statuses.insert(key, status);
            }
        }

        // Change detection badge — any non-empty delta on inactive tab
        let has_data_changes = delta.changes.iter().any(|c| {
            !matches!(
                c,
                flotilla_protocol::Change::ProviderHealth { .. }
                    | flotilla_protocol::Change::ErrorsChanged(_)
            )
        });
        if has_data_changes {
            let active_idx = self.model.active_repo;
            let i = self.model.repo_order.iter().position(|p| p == &path);
            if let Some(idx) = i {
                if idx != active_idx {
                    if let Some(rui) = self.ui.repo_ui.get_mut(&path) {
                        rui.has_unseen_changes = true;
                    }
                }
            }
        }

        if let Some(rui) = self.ui.repo_ui.get_mut(&path) {
            rui.update_table_view(table_view);
        }
    }

    fn handle_repo_added(&mut self, info: RepoInfo) {
        let path = info.path.clone();
        if self.model.repos.contains_key(&path) {
            return;
        }
        self.model.repos.insert(
            path.clone(),
            TuiRepoModel {
                providers: Arc::new(ProviderData::default()),
                labels: info.labels,
                provider_names: info.provider_names,
                provider_health: info.provider_health,
                loading: info.loading,
                issue_has_more: false,
                issue_total: None,
                issue_search_active: false,
                issue_fetch_pending: false,
                issue_initial_requested: false,
            },
        );
        self.model.repo_order.push(path.clone());
        self.ui.repo_ui.insert(path, RepoUiState::default());
        self.switch_tab(self.model.repo_order.len() - 1);
    }

    fn handle_repo_removed(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.model.repos.remove(&path);
        self.model.repo_order.retain(|p| p != &path);
        self.ui.repo_ui.remove(&path);
        if self.model.repo_order.is_empty() {
            self.should_quit = true;
            return;
        }
        if self.model.active_repo >= self.model.repo_order.len() {
            self.model.active_repo = self.model.repo_order.len() - 1;
        }
    }

    // ── Convenience accessors ──

    pub fn active_ui(&self) -> &RepoUiState {
        self.ui
            .active_repo_ui(&self.model.repo_order, self.model.active_repo)
    }

    pub fn active_ui_mut(&mut self) -> &mut RepoUiState {
        let key = &self.model.repo_order[self.model.active_repo];
        self.ui
            .repo_ui
            .get_mut(key)
            .expect("active repo must have UI state")
    }

    pub fn selected_work_item(&self) -> Option<&WorkItem> {
        let table_idx = self.active_ui().table_state.selected()?;
        match self.active_ui().table_view.table_entries.get(table_idx)? {
            GroupEntry::Item(item) => Some(item),
            GroupEntry::Header(_) => None,
        }
    }

    pub fn prefill_branch_input(
        &mut self,
        branch_name: &str,
        pending_issue_ids: Vec<(String, String)>,
    ) {
        self.ui.mode = UiMode::BranchInput {
            input: Input::from(branch_name),
            generating: false,
            pending_issue_ids,
        };
    }

    // ── File picker ──

    fn handle_file_picker_key(&mut self, key: KeyEvent) {
        // Keys that change mode
        if key.code == KeyCode::Esc {
            self.ui.mode = UiMode::Normal;
            return;
        }
        if key.code == KeyCode::Enter {
            self.activate_dir_entry();
            return;
        }

        let needs_refresh = {
            let UiMode::FilePicker {
                ref mut input,
                ref mut dir_entries,
                ref mut selected,
            } = self.ui.mode
            else {
                return;
            };
            match key.code {
                KeyCode::Down | KeyCode::Char('j')
                    if key.modifiers.is_empty() || key.code == KeyCode::Down =>
                {
                    if !dir_entries.is_empty() {
                        *selected = (*selected + 1).min(dir_entries.len() - 1);
                    }
                    false
                }
                KeyCode::Up | KeyCode::Char('k')
                    if key.modifiers.is_empty() || key.code == KeyCode::Up =>
                {
                    *selected = selected.saturating_sub(1);
                    false
                }
                KeyCode::Tab => {
                    if let Some(entry) = dir_entries.get(*selected).cloned() {
                        let current = input.value().to_string();
                        let base = if current.ends_with('/') {
                            current.clone()
                        } else {
                            current
                                .rsplit_once('/')
                                .map(|(prefix, _)| format!("{prefix}/"))
                                .unwrap_or_default()
                        };
                        let new_path = format!("{}{}/", base, entry.name);
                        *input = Input::from(new_path.as_str());
                        *selected = 0;
                    }
                    true
                }
                _ => {
                    input.handle_event(&crossterm::event::Event::Key(key));
                    *selected = 0;
                    true
                }
            }
        };
        if needs_refresh {
            self.refresh_dir_listing();
        }
    }

    fn activate_dir_entry(&mut self) {
        let (entry, base) = {
            let UiMode::FilePicker {
                ref input,
                ref dir_entries,
                selected,
            } = self.ui.mode
            else {
                return;
            };
            let Some(entry) = dir_entries.get(selected).cloned() else {
                return;
            };
            let current = input.value().to_string();
            let base = if current.ends_with('/') {
                current
            } else {
                current
                    .rsplit_once('/')
                    .map(|(prefix, _)| format!("{prefix}/"))
                    .unwrap_or_default()
            };
            (entry, base)
        };

        if entry.is_git_repo && !entry.is_added {
            let path = PathBuf::from(format!("{}{}", base, entry.name));
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            self.proto_commands
                .push(Command::AddRepo { path: canonical });
            self.ui.mode = UiMode::Normal;
        } else if entry.is_dir {
            let new_path = format!("{}{}/", base, entry.name);
            if let UiMode::FilePicker {
                ref mut input,
                ref mut selected,
                ..
            } = self.ui.mode
            {
                *input = Input::from(new_path.as_str());
                *selected = 0;
            }
            self.refresh_dir_listing();
        }
    }

    pub fn handle_file_picker_mouse(&mut self, mouse: MouseEvent) {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        let x = mouse.column;
        let y = mouse.row;
        let a = self.ui.layout.file_picker_area;
        if x < a.x || x >= a.x + a.width || y < a.y || y >= a.y + a.height {
            self.ui.mode = UiMode::Normal;
            return;
        }
        let la = self.ui.layout.file_picker_list_area;
        if x >= la.x && x < la.x + la.width && y >= la.y && y < la.y + la.height {
            let row = (y - la.y) as usize;
            let len = if let UiMode::FilePicker {
                ref dir_entries, ..
            } = self.ui.mode
            {
                dir_entries.len()
            } else {
                return;
            };
            if row < len {
                if let UiMode::FilePicker {
                    ref mut selected, ..
                } = self.ui.mode
                {
                    *selected = row;
                }
                self.activate_dir_entry();
            }
        }
    }

    pub fn refresh_dir_listing(&mut self) {
        let Self { model, ui, .. } = self;
        let UiMode::FilePicker {
            ref input,
            ref mut dir_entries,
            ..
        } = ui.mode
        else {
            return;
        };

        let path_str = input.value().to_string();
        let dir = if path_str.ends_with('/') {
            PathBuf::from(&path_str)
        } else {
            PathBuf::from(&path_str)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        };

        let filter = if !path_str.ends_with('/') {
            PathBuf::from(&path_str)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if !filter.is_empty() && !name.to_lowercase().starts_with(&filter) {
                    continue;
                }
                let path = entry.path();
                let is_dir = path.is_dir();
                if !is_dir {
                    continue;
                }
                let is_git_repo = path.join(".git").exists();
                let canonical = std::fs::canonicalize(&path).unwrap_or(path);
                let is_added = model.repos.contains_key(&canonical);
                entries.push(DirEntry {
                    name,
                    is_dir,
                    is_git_repo,
                    is_added,
                });
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        *dir_entries = entries;
    }
}
