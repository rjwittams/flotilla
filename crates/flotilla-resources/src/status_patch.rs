use crate::{
    error::ResourceError,
    resource::{Resource, ResourceObject},
    TypedResolver,
};

const MAX_RETRIES: usize = 3;

pub trait StatusPatch<S>: Send + Sync {
    fn apply(&self, status: &mut S);
}

pub enum NoStatusPatch {}

impl StatusPatch<()> for NoStatusPatch {
    fn apply(&self, _: &mut ()) {
        match *self {}
    }
}

pub async fn apply_status_patch<T>(
    resolver: &TypedResolver<T>,
    name: &str,
    patch: &T::StatusPatch,
) -> Result<ResourceObject<T>, ResourceError>
where
    T: Resource,
    T::Status: Default,
{
    for _ in 0..MAX_RETRIES {
        let current = resolver.get(name).await?;
        let mut new_status = current.status.clone().unwrap_or_default();
        patch.apply(&mut new_status);
        match resolver.update_status(name, &current.metadata.resource_version, &new_status).await {
            Ok(updated) => return Ok(updated),
            Err(ResourceError::Conflict { .. }) => continue,
            Err(other) => return Err(other),
        }
    }

    Err(ResourceError::conflict(name, "status patch retry budget exhausted"))
}
