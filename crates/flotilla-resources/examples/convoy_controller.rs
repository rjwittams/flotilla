use std::{env, path::PathBuf, time::Duration};

use flotilla_resources::{
    controller::ControllerLoop, ensure_crd, ensure_namespace, Convoy, ConvoyReconciler, HttpBackend, ResourceBackend, TaskWorkspace,
    WorkflowTemplate,
};
use tracing::info;

fn kubeconfig_path() -> PathBuf {
    if let Ok(path) = env::var("KUBECONFIG") {
        return PathBuf::from(path);
    }
    let home = env::var("HOME").expect("HOME must be set when KUBECONFIG is unset");
    PathBuf::from(home).join(".kube/config")
}

fn parse_namespace() -> String {
    let mut args = env::args().skip(1);
    let mut namespace = "flotilla".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--namespace" => {
                namespace = args.next().expect("--namespace requires a value");
            }
            other => panic!("unexpected argument: {other}"),
        }
    }
    namespace
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_target(false).init();

    let namespace = parse_namespace();
    let backend = HttpBackend::from_kubeconfig(kubeconfig_path())?;
    ensure_namespace(&backend, &namespace).await?;
    ensure_crd(&backend, include_str!("../src/crds/workflow_template.crd.yaml")).await?;
    ensure_crd(&backend, include_str!("../src/crds/convoy.crd.yaml")).await?;
    ensure_crd(&backend, include_str!("../src/crds/task_workspace.crd.yaml")).await?;

    let backend = ResourceBackend::Http(backend);
    let convoys = backend.clone().using::<Convoy>(&namespace);
    let templates = backend.clone().using::<WorkflowTemplate>(&namespace);
    let task_workspaces = backend.clone().using::<TaskWorkspace>(&namespace);

    info!("starting convoy controller loop");
    ControllerLoop {
        primary: convoys,
        secondaries: ConvoyReconciler::secondary_watches(),
        reconciler: ConvoyReconciler::new(templates).with_task_workspaces(task_workspaces),
        resync_interval: Duration::from_secs(60),
        backend,
    }
    .run()
    .await?;

    Ok(())
}
