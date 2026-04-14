use std::{env, fs, path::PathBuf, process::ExitCode};

use flotilla_resources::{validate, WorkflowTemplateSpec};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct WorkflowTemplateDocument {
    spec: WorkflowTemplateSpec,
}

fn load_spec(path: &PathBuf) -> Result<WorkflowTemplateSpec, String> {
    let yaml = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;

    serde_yml::from_str::<WorkflowTemplateDocument>(&yaml).map(|document| document.spec).or_else(|document_error| {
        serde_yml::from_str::<WorkflowTemplateSpec>(&yaml).map_err(|spec_error| {
            format!(
                "parse {} as workflow template document failed: {document_error}; parse as bare spec failed: {spec_error}",
                path.display()
            )
        })
    })
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cargo run -p flotilla-resources --example validate_workflow -- <path>");
        return ExitCode::from(2);
    };

    let spec = match load_spec(&path) {
        Ok(spec) => spec,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    match validate(&spec) {
        Ok(()) => {
            println!("workflow template is valid");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            eprintln!("workflow template validation failed:");
            for error in errors {
                eprintln!("- {error:?}");
            }
            ExitCode::FAILURE
        }
    }
}
