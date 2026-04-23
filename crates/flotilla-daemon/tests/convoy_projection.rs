//! Integration test: ConvoyProjection wired into DaemonRuntime.
//!
//! Verifies that creating a Convoy resource causes a NamespaceSnapshot event to
//! reach subscribed clients through the daemon's broadcast event bus.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use flotilla_core::{
    config::ConfigStore, daemon::DaemonHandle, in_process::InProcessDaemon, providers::discovery::test_support::fake_discovery,
};
use flotilla_daemon::runtime::{DaemonRuntime, RuntimeOptions};
use flotilla_protocol::{DaemonEvent, HostName};
use flotilla_resources::{ConvoySpec, InMemoryBackend, InputMeta, ResourceBackend};

fn test_config(dir: std::path::PathBuf) -> Arc<ConfigStore> {
    std::fs::create_dir_all(&dir).expect("create config dir");
    std::fs::write(dir.join("daemon.toml"), "machine_id = \"test-convoy\"\n").expect("write daemon config");
    Arc::new(ConfigStore::with_base(dir))
}

#[tokio::test]
async fn convoy_projection_emits_namespace_events() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let config = test_config(tmp.path().join("config"));

    let backend = ResourceBackend::InMemory(InMemoryBackend::default());
    let daemon = InProcessDaemon::new_with_resource_backend(
        vec![],
        Arc::clone(&config),
        fake_discovery(false),
        HostName::new("local"),
        backend.clone(),
    )
    .await;

    // Subscribe before starting the runtime so we don't miss the first event.
    let mut rx = daemon.subscribe();

    // Start with fast resync to avoid test flakiness.
    let options = RuntimeOptions {
        namespace: "flotilla".to_string(),
        heartbeat_interval: Duration::from_secs(300),
        controller_resync_interval: Duration::from_secs(300),
        start_controllers: true,
    };
    let _runtime = DaemonRuntime::start_with_options(Arc::clone(&daemon), Arc::clone(&config), None, options).await.expect("runtime start");

    // Create a Convoy resource — the projection should pick it up via the watch
    // stream and emit a NamespaceSnapshot for "flotilla".
    let convoys = backend.using::<flotilla_resources::Convoy>("flotilla");
    let meta = InputMeta {
        name: "test-convoy-1".to_string(),
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        owner_references: vec![],
        finalizers: vec![],
        deletion_timestamp: None,
    };
    let spec = ConvoySpec {
        workflow_ref: "my-workflow".to_string(),
        inputs: BTreeMap::new(),
        placement_policy: None,
        repository: None,
        r#ref: None,
    };
    convoys.create(&meta, &spec).await.expect("create convoy");

    // Wait for a NamespaceSnapshot for the "flotilla" namespace.
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(DaemonEvent::NamespaceSnapshot(snap)) if snap.namespace == "flotilla" => {
                    return snap;
                }
                Ok(_) => continue,
                Err(err) => panic!("broadcast receive error: {err}"),
            }
        }
    })
    .await
    .expect("timed out waiting for NamespaceSnapshot for 'flotilla' namespace");

    assert_eq!(found.namespace, "flotilla");
    assert_eq!(found.convoys.len(), 1, "expected exactly one convoy in the snapshot");
    assert_eq!(found.convoys[0].name, "test-convoy-1");
}
