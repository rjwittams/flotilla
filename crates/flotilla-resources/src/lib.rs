mod backend;
mod error;
mod http;
mod in_memory;
mod resource;
mod status_patch;
mod watch;
mod workflow_template;

pub use backend::{ResourceBackend, TypedResolver};
pub use error::ResourceError;
pub use http::{ensure_crd, ensure_namespace, HttpBackend};
pub use in_memory::InMemoryBackend;
pub use resource::{ApiPaths, InputMeta, ObjectMeta, Resource, ResourceObject};
pub use status_patch::{apply_status_patch, NoStatusPatch, StatusPatch};
pub use watch::{ResourceList, WatchEvent, WatchStart, WatchStream};
pub use workflow_template::{
    validate, InputDefinition, InterpolationField, InterpolationLocation, ProcessDefinition, ProcessSource, Selector, TaskDefinition,
    ValidationError, WorkflowTemplate, WorkflowTemplateSpec,
};
