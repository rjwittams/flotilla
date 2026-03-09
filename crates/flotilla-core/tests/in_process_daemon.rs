use std::sync::Arc;

use flotilla_core::config::ConfigStore;
use flotilla_core::daemon::DaemonHandle;
use flotilla_core::in_process::InProcessDaemon;
use flotilla_protocol::{Command, DaemonEvent};

#[tokio::test]
async fn daemon_broadcasts_snapshots() {
    let repo = std::env::current_dir().unwrap();
    let config = Arc::new(ConfigStore::new());
    let daemon = InProcessDaemon::new(vec![repo.clone()], config).await;
    let mut rx = daemon.subscribe();

    let event = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("timeout waiting for snapshot")
        .expect("recv error");

    match event {
        DaemonEvent::Snapshot(snap) => {
            assert_eq!(snap.repo, repo);
            assert!(snap.seq > 0);
        }
        other => panic!("expected Snapshot, got {:?}", other),
    }
}

#[tokio::test]
async fn execute_broadcasts_lifecycle_events() {
    let repo = std::env::current_dir().unwrap();
    let config = Arc::new(ConfigStore::new());
    let daemon = InProcessDaemon::new(vec![repo.clone()], config).await;
    let mut rx = daemon.subscribe();

    // Execute a command that goes through the spawned task path.
    // GenerateBranchName with empty issue_keys will hit the fallback (no AI
    // provider) and return BranchNameGenerated with an empty name — we only
    // care about the lifecycle events, not the command result.
    let command = Command::GenerateBranchName { issue_keys: vec![] };
    let command_id = daemon
        .execute(&repo, command)
        .await
        .expect("execute should return a command id");

    // Collect CommandStarted and CommandFinished events, skipping any
    // Snapshot events that arrive from the background refresh loop.
    let timeout = std::time::Duration::from_secs(10);
    let mut got_started = false;
    let mut got_finished = false;
    let mut started_id = None;
    let mut finished_id = None;

    let result = tokio::time::timeout(timeout, async {
        while !got_started || !got_finished {
            match rx.recv().await {
                Ok(DaemonEvent::CommandStarted {
                    command_id: id,
                    repo: ref event_repo,
                    ..
                }) => {
                    assert_eq!(
                        event_repo, &repo,
                        "CommandStarted repo should match executed repo"
                    );
                    started_id = Some(id);
                    got_started = true;
                }
                Ok(DaemonEvent::CommandFinished {
                    command_id: id,
                    repo: ref event_repo,
                    ..
                }) => {
                    assert_eq!(
                        event_repo, &repo,
                        "CommandFinished repo should match executed repo"
                    );
                    finished_id = Some(id);
                    got_finished = true;
                }
                Ok(_) => {
                    // Skip snapshot and other events
                }
                Err(e) => panic!("unexpected recv error: {:?}", e),
            }
        }
    })
    .await;

    result.expect("timed out waiting for lifecycle events");

    // Both events must carry the same command ID returned by execute()
    assert_eq!(
        started_id,
        Some(command_id),
        "CommandStarted id should match the id returned by execute()"
    );
    assert_eq!(
        finished_id,
        Some(command_id),
        "CommandFinished id should match the id returned by execute()"
    );
}
