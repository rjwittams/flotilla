#![allow(dead_code)]

use std::{collections::BTreeMap, future::Future, time::Duration};

use flotilla_resources::{canonicalize_repo_url, repo_key, InputMeta};
use tokio::time::{sleep, Instant};

pub fn meta(name: &str) -> InputMeta {
    InputMeta {
        name: name.to_string(),
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        owner_references: Vec::new(),
        finalizers: Vec::new(),
        deletion_timestamp: None,
    }
}

pub fn labeled_meta(name: &str, labels: impl IntoIterator<Item = (String, String)>) -> InputMeta {
    let mut meta = meta(name);
    meta.labels = labels.into_iter().collect();
    meta
}

pub fn task_workspace_meta(name: &str, repo_url: &str) -> InputMeta {
    let canonical_repo = canonicalize_repo_url(repo_url).expect("repo URL should canonicalize");
    labeled_meta(name, [("flotilla.work/repo-key".to_string(), repo_key(&canonical_repo))])
}

#[allow(dead_code)]
pub async fn wait_until<F, Fut>(timeout: Duration, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition().await {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("condition was not satisfied within {:?}", timeout);
}
