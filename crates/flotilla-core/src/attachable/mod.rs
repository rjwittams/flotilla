pub mod store;
pub mod types;

pub use store::{AttachableRegistry, AttachableStore, SharedAttachableStore};
pub use types::{
    Attachable, AttachableContent, AttachableId, AttachableSet, AttachableSetId, BindingObjectKind, ProviderBinding, TerminalAttachable,
    TerminalPurpose,
};
