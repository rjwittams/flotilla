use std::{env, path::PathBuf};

use flotilla_resources::{
    ensure_crd, ensure_namespace, validate, HttpBackend, InputMeta, ResourceBackend, WatchEvent, WatchStart, WorkflowTemplate,
    WorkflowTemplateSpec,
};
use futures::StreamExt;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct WorkflowTemplateDocument {
    spec: WorkflowTemplateSpec,
}

fn kubeconfig_path() -> PathBuf {
    if let Ok(path) = env::var("KUBECONFIG") {
        return PathBuf::from(path);
    }
    let home = env::var("HOME").expect("HOME must be set when KUBECONFIG is unset");
    PathBuf::from(home).join(".kube/config")
}

fn sample_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/review-and-fix.yaml")
}

fn load_workflow_spec() -> Result<WorkflowTemplateSpec, Box<dyn std::error::Error>> {
    let yaml = std::fs::read_to_string(sample_workflow_path())?;
    let document: WorkflowTemplateDocument = serde_yml::from_str(&yaml)?;
    Ok(document.spec)
}

fn updated_workflow_spec() -> WorkflowTemplateSpec {
    let mut spec = load_workflow_spec().expect("sample workflow should parse");
    if let flotilla_resources::ProcessSource::Tool { command } = &mut spec.tasks[0].processes[1].source {
        *command = "cargo check --all-targets".to_string();
    }
    spec
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kubeconfig = kubeconfig_path();
    let backend = HttpBackend::from_kubeconfig(&kubeconfig)?;
    let namespace = "flotilla";
    ensure_namespace(&backend, namespace).await?;
    ensure_crd(&backend, include_str!("../src/crds/convoy.crd.yaml")).await?;
    ensure_crd(&backend, include_str!("../src/crds/workflow_template.crd.yaml")).await?;

    let resolver = ResourceBackend::Http(backend).using::<WorkflowTemplate>(namespace);
    let spec = load_workflow_spec()?;
    validate(&spec).map_err(|errors| format!("sample workflow failed validation: {errors:?}"))?;
    let meta = InputMeta {
        name: format!("demo-workflow-template-{}", std::process::id()),
        labels: [("app".to_string(), "flotilla".to_string())].into_iter().collect(),
        annotations: Default::default(),
    };
    if let Err(err) = resolver.delete(&meta.name).await {
        if !matches!(err, flotilla_resources::ResourceError::NotFound { .. }) {
            return Err(err.into());
        }
    }

    println!("creating resource");
    let created = resolver.create(&meta, &spec).await?;
    println!("created {} rv={}", created.metadata.name, created.metadata.resource_version);

    let listed = resolver.list().await?;
    println!("listed {} resources at rv={}", listed.items.len(), listed.resource_version);

    let mut watch = resolver.watch(WatchStart::FromVersion(listed.resource_version.clone())).await?;
    println!("updating spec");
    let updated = resolver.update(&meta, &created.metadata.resource_version, &updated_workflow_spec()).await?;
    println!("updated rv={}", updated.metadata.resource_version);

    if let Some(event) = watch.next().await {
        match event? {
            WatchEvent::Modified(object) => {
                println!("watch saw modified resource rv={}", object.metadata.resource_version);
            }
            WatchEvent::Added(_) | WatchEvent::Deleted(_) => println!("watch saw non-modified event"),
        }
    }

    println!("deleting resource");
    resolver.delete(&created.metadata.name).await?;
    if let Some(event) = watch.next().await {
        match event? {
            WatchEvent::Deleted(object) => println!("watch saw deleted resource rv={}", object.metadata.resource_version),
            WatchEvent::Added(_) | WatchEvent::Modified(_) => println!("watch saw non-deleted event"),
        }
    }
    Ok(())
}
