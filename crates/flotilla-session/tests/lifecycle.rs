use std::path::PathBuf;

use clap::Parser;
use flotilla_session::{
    cli::{self, Cli},
    protocol::SessionInfo,
    runtime::RuntimeLayout,
    server::SessionService,
    session::daemon_pid_path,
};

fn service_for(path: &std::path::Path) -> SessionService {
    SessionService::new(RuntimeLayout::new(path.to_path_buf()))
}

#[test]
fn create_makes_session_directory_and_returns_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = service_for(temp.path());
    let cli = Cli::try_parse_from(["flotilla-session", "create", "--name", "alpha", "--cmd", "bash"]).expect("parse create");

    let output = cli::execute(cli, &service).expect("execute create").expect("create output");
    let created: SessionInfo = serde_json::from_str(&output).expect("parse create output");

    assert_eq!(created.id, "alpha");
    assert!(temp.path().join("alpha").exists());
    assert_eq!(created.cmd.as_deref(), Some("bash"));
}

#[test]
fn list_reports_existing_sessions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = service_for(temp.path());
    service.create(Some("alpha".into()), Some(PathBuf::from("/repo")), None).expect("create alpha");
    service.create(Some("beta".into()), None, Some("zsh".into())).expect("create beta");
    let cli = Cli::try_parse_from(["flotilla-session", "list"]).expect("parse list");

    let output = cli::execute(cli, &service).expect("execute list").expect("list output");
    let listed: Vec<SessionInfo> = serde_json::from_str(&output).expect("parse list output");

    assert_eq!(listed.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), vec!["alpha", "beta"]);
}

#[test]
fn kill_removes_session_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = service_for(temp.path());
    service.create(Some("alpha".into()), None, None).expect("create alpha");
    let cli = Cli::try_parse_from(["flotilla-session", "kill", "alpha"]).expect("parse kill");

    let output = cli::execute(cli, &service).expect("execute kill");

    assert_eq!(output, None);
    assert!(!temp.path().join("alpha").exists());
}

#[test]
fn attach_creates_session_lazily_and_reuses_it_on_later_attach() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = service_for(temp.path());

    let (first, attach) = service.attach(Some("alpha".into()), None, Some("sleep 5".into())).expect("first attach");
    assert_eq!(first.id, "alpha");
    assert!(daemon_pid_path(temp.path(), "alpha").exists());

    drop(attach);

    let (second, _attach2) = service.attach(Some("alpha".into()), None, None).expect("reattach");
    assert_eq!(second.id, "alpha");
}

#[test]
fn attach_rejects_second_foreground_client_while_one_is_active() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = service_for(temp.path());

    let (_session, _attach) = service.attach(Some("alpha".into()), None, Some("sleep 5".into())).expect("first attach");
    let err = service.attach(Some("alpha".into()), None, None).expect_err("second attach should fail");

    assert!(err.contains("foreground client"));
}

#[test]
fn dropping_foreground_attach_keeps_session_alive_for_later_attach() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = service_for(temp.path());

    let (_session, attach) = service.attach(Some("alpha".into()), None, Some("sleep 5".into())).expect("first attach");
    let pid_path = daemon_pid_path(temp.path(), "alpha");
    assert!(pid_path.exists());

    drop(attach);

    let (_session, _reattach) = service.attach(Some("alpha".into()), None, None).expect("reattach after disconnect");
    assert!(pid_path.exists());
}
