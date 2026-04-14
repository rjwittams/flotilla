mod common;

use std::time::Duration;

use common::{updated_workflow_template_spec, valid_workflow_template_spec, workflow_template_meta};
use flotilla_resources::{InMemoryBackend, ResourceBackend, WatchEvent, WatchStart, WorkflowTemplate};
use futures::StreamExt;
use tokio::time::timeout;

fn resolver(namespace: &str) -> flotilla_resources::TypedResolver<WorkflowTemplate> {
    ResourceBackend::InMemory(InMemoryBackend::default()).using::<WorkflowTemplate>(namespace)
}

#[tokio::test]
async fn create_get_list_roundtrip_for_workflow_templates() {
    let resolver = resolver("flotilla");
    let created =
        resolver.create(&workflow_template_meta("review-and-fix"), &valid_workflow_template_spec()).await.expect("create should succeed");

    assert_eq!(created.metadata.name, "review-and-fix");
    assert_eq!(created.metadata.namespace, "flotilla");
    assert!(created.status.is_none(), "workflow templates do not use status");
    assert_eq!(created.spec.tasks.len(), 2);

    let fetched = resolver.get("review-and-fix").await.expect("get should succeed");
    assert_eq!(fetched.metadata.resource_version, created.metadata.resource_version);

    let listed = resolver.list().await.expect("list should succeed");
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].metadata.name, "review-and-fix");
}

#[tokio::test]
async fn update_requires_current_resource_version_for_workflow_templates() {
    let resolver = resolver("flotilla");
    let created =
        resolver.create(&workflow_template_meta("review-and-fix"), &valid_workflow_template_spec()).await.expect("create should succeed");

    let conflict = resolver
        .update(&workflow_template_meta("review-and-fix"), "0", &updated_workflow_template_spec())
        .await
        .expect_err("stale update should conflict");
    match conflict {
        flotilla_resources::ResourceError::Conflict { .. } => {}
        other => panic!("expected conflict, got {other}"),
    }

    let updated = resolver
        .update(&workflow_template_meta("review-and-fix"), &created.metadata.resource_version, &updated_workflow_template_spec())
        .await
        .expect("update should succeed");

    assert_eq!(updated.metadata.resource_version, "2");
    match &updated.spec.tasks[0].processes[1].source {
        flotilla_resources::ProcessSource::Tool { command } => assert_eq!(command, "cargo check --all-targets"),
        other => panic!("expected tool process, got {other:?}"),
    }
}

#[tokio::test]
async fn delete_emits_deleted_event_for_workflow_templates() {
    let resolver = resolver("flotilla");
    let created =
        resolver.create(&workflow_template_meta("review-and-fix"), &valid_workflow_template_spec()).await.expect("create should succeed");
    let mut watch = resolver.watch(WatchStart::FromVersion(created.metadata.resource_version.clone())).await.expect("watch should succeed");

    resolver.delete("review-and-fix").await.expect("delete should succeed");
    let event = timeout(Duration::from_secs(1), watch.next())
        .await
        .expect("watch should produce delete event")
        .expect("stream should yield item")
        .expect("event should decode");

    match event {
        WatchEvent::Deleted(object) => {
            assert_eq!(object.metadata.name, "review-and-fix");
            assert_eq!(object.metadata.resource_version, "2");
        }
        other => panic!("expected deleted event, got {other:?}"),
    }
}

#[tokio::test]
async fn watch_from_version_replays_update_then_delete_for_workflow_templates() {
    let resolver = resolver("flotilla");
    resolver.create(&workflow_template_meta("review-and-fix"), &valid_workflow_template_spec()).await.expect("create should succeed");

    let listed = resolver.list().await.expect("list should succeed");
    let mut watch = resolver.watch(WatchStart::FromVersion(listed.resource_version.clone())).await.expect("watch should succeed");

    let updated = resolver
        .update(&workflow_template_meta("review-and-fix"), &listed.items[0].metadata.resource_version, &updated_workflow_template_spec())
        .await
        .expect("update should succeed");

    let modified = timeout(Duration::from_secs(1), watch.next())
        .await
        .expect("watch should produce modified event")
        .expect("stream should yield item")
        .expect("event should decode");
    match modified {
        WatchEvent::Modified(object) => assert_eq!(object.metadata.resource_version, updated.metadata.resource_version),
        other => panic!("expected modified event, got {other:?}"),
    }

    resolver.delete("review-and-fix").await.expect("delete should succeed");
    let deleted = timeout(Duration::from_secs(1), watch.next())
        .await
        .expect("watch should produce deleted event")
        .expect("stream should yield item")
        .expect("event should decode");
    match deleted {
        WatchEvent::Deleted(object) => assert_eq!(object.metadata.resource_version, "3"),
        other => panic!("expected deleted event, got {other:?}"),
    }
}
