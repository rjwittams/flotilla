use crate::{
    protocol::{SessionInfo, SessionStatus},
    runtime::{RuntimeLayout, SessionRecord},
    session::{
        attach_foreground, daemon_pid_path, ensure_session_started, foreground_path, run_session_daemon, session_socket_path,
        ForegroundAttach,
    },
};

#[derive(Debug, Clone)]
pub struct SessionService {
    layout: RuntimeLayout,
}

impl SessionService {
    pub fn new(layout: RuntimeLayout) -> Self {
        Self { layout }
    }

    pub fn discover() -> Self {
        Self::new(RuntimeLayout::discover())
    }

    pub fn create(&self, name: Option<String>, cwd: Option<std::path::PathBuf>, cmd: Option<String>) -> Result<SessionInfo, String> {
        let session = ensure_session_started(&self.layout, name, cwd, cmd)?;
        Ok(SessionInfo { id: session.id, name: session.name, cwd: session.cwd, cmd: session.cmd, status: SessionStatus::Disconnected })
    }

    pub fn list(&self) -> Result<Vec<SessionInfo>, String> {
        self.layout
            .list_sessions()
            .map(|sessions| sessions.into_iter().map(|record| session_info_from_record(self.layout.root(), record)).collect())
    }

    pub fn kill(&self, id: &str) -> Result<(), String> {
        let pid_path = crate::session::daemon_pid_path(self.layout.root(), id);
        if let Ok(pid) = std::fs::read_to_string(&pid_path).map(|value| value.trim().parse::<i32>().ok()) {
            if let Some(pid) = pid {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
        self.layout.remove_session(id)
    }

    pub fn attach(
        &self,
        name: Option<String>,
        cwd: Option<std::path::PathBuf>,
        cmd: Option<String>,
    ) -> Result<(SessionInfo, ForegroundAttach), String> {
        let session = ensure_session_started(&self.layout, name, cwd, cmd)?;
        let attach = attach_foreground(&self.layout, &session.id)?;
        Ok((SessionInfo { id: session.id, name: session.name, cwd: session.cwd, cmd: session.cmd, status: SessionStatus::Running }, attach))
    }

    pub fn serve(&self, id: &str) -> Result<(), String> {
        run_session_daemon(self.layout.root(), id)
    }
}

fn session_info_from_record(root: &std::path::Path, record: SessionRecord) -> SessionInfo {
    let id = record.metadata.id.clone();
    let status = if foreground_path(root, &id).exists() {
        SessionStatus::Running
    } else if session_socket_path(root, &id).exists() || daemon_pid_path(root, &id).exists() {
        SessionStatus::Disconnected
    } else {
        SessionStatus::Disconnected
    };
    SessionInfo { id: record.metadata.id, name: record.metadata.name, cwd: record.metadata.cwd, cmd: record.metadata.cmd, status }
}
