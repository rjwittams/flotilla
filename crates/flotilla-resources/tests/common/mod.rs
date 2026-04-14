#![allow(dead_code)]

use flotilla_resources::{
    ApiPaths, InputDefinition, InputMeta, ProcessDefinition, ProcessSource, Resource, Selector, TaskDefinition, WorkflowTemplateSpec,
};
use serde::{Deserialize, Serialize};

pub struct ConvoyResource;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvoySpec {
    pub template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvoyStatus {
    pub phase: String,
}

impl Resource for ConvoyResource {
    type Spec = ConvoySpec;
    type Status = ConvoyStatus;

    const API_PATHS: ApiPaths = ApiPaths { group: "flotilla.work", version: "v1", plural: "convoys", kind: "Convoy" };
}

pub fn input_meta(name: &str) -> InputMeta {
    InputMeta {
        name: name.to_string(),
        labels: [("app".to_string(), "flotilla".to_string())].into_iter().collect(),
        annotations: [("note".to_string(), "test".to_string())].into_iter().collect(),
    }
}

pub fn spec(template: &str) -> ConvoySpec {
    ConvoySpec { template: template.to_string() }
}

pub fn status(phase: &str) -> ConvoyStatus {
    ConvoyStatus { phase: phase.to_string() }
}

pub fn workflow_template_meta(name: &str) -> InputMeta {
    InputMeta {
        name: name.to_string(),
        labels: [("app".to_string(), "flotilla".to_string())].into_iter().collect(),
        annotations: [("note".to_string(), "workflow-template-test".to_string())].into_iter().collect(),
    }
}

pub fn valid_workflow_template_spec() -> WorkflowTemplateSpec {
    WorkflowTemplateSpec {
        inputs: vec![
            InputDefinition { name: "feature".to_string(), description: Some("Brief description of the feature to implement".to_string()) },
            InputDefinition { name: "branch".to_string(), description: Some("Target git branch".to_string()) },
        ],
        tasks: vec![
            TaskDefinition {
                name: "implement".to_string(),
                depends_on: Vec::new(),
                processes: vec![
                    ProcessDefinition {
                        role: "coder".to_string(),
                        source: ProcessSource::Agent {
                            selector: Selector { capability: "code".to_string() },
                            prompt: Some(
                                "Convoy {{workflow.name}} - implement {{inputs.feature}} on branch {{inputs.branch}}.".to_string(),
                            ),
                        },
                    },
                    ProcessDefinition {
                        role: "build".to_string(),
                        source: ProcessSource::Tool { command: "cargo watch -x check".to_string() },
                    },
                ],
            },
            TaskDefinition {
                name: "review".to_string(),
                depends_on: vec!["implement".to_string()],
                processes: vec![
                    ProcessDefinition {
                        role: "reviewer".to_string(),
                        source: ProcessSource::Agent {
                            selector: Selector { capability: "code-review".to_string() },
                            prompt: Some("Review branch {{inputs.branch}} for correctness and style.".to_string()),
                        },
                    },
                    ProcessDefinition {
                        role: "tests".to_string(),
                        source: ProcessSource::Tool { command: "cargo test --watch".to_string() },
                    },
                ],
            },
        ],
    }
}

pub fn updated_workflow_template_spec() -> WorkflowTemplateSpec {
    let mut spec = valid_workflow_template_spec();
    if let ProcessSource::Tool { command } = &mut spec.tasks[0].processes[1].source {
        *command = "cargo check --all-targets".to_string();
    }
    spec
}

pub fn valid_workflow_template_yaml() -> &'static str {
    include_str!("../../examples/review-and-fix.yaml")
}
