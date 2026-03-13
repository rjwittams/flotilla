use std::{future::Future, pin::Pin};

use flotilla_protocol::CommandResult;

/// Outcome of a single step execution.
pub enum StepOutcome {
    /// Step completed successfully, no specific result to report.
    Completed,
    /// Step completed and wants to override the final CommandResult.
    CompletedWith(CommandResult),
    /// Step determined its work was already done and skipped.
    Skipped,
}

/// A single step in a multi-step command.
pub struct Step {
    pub description: String,
    pub action: Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<StepOutcome, String>> + Send>> + Send>,
}

/// A plan of steps to execute for a command.
pub struct StepPlan {
    pub steps: Vec<Step>,
}

impl StepPlan {
    pub fn new(steps: Vec<Step>) -> Self {
        Self { steps }
    }
}
