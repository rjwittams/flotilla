use std::sync::Arc;
use crate::data::WorkItem;
use crate::provider_data::ProviderData;
use crate::providers::correlation::CorrelatedGroup;

#[derive(Debug, Clone)]
pub struct CorrelatedData {
    pub provider_data: Arc<ProviderData>,
    pub work_items: Vec<WorkItem>,
    pub correlation_groups: Vec<CorrelatedGroup>,
}
