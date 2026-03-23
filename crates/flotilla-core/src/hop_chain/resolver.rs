use super::{Hop, ResolutionContext};

/// Decides whether to wrap (nest inner command as argument) or sendkeys
/// (create an execution boundary) at each combination point during resolution.
pub trait CombineStrategy: Send + Sync {
    fn should_wrap(&self, hop: &Hop, context: &ResolutionContext) -> bool;
}

/// Always nests commands as arguments. Matches current SSH wrapping behavior. Default.
pub struct AlwaysWrap;

impl CombineStrategy for AlwaysWrap {
    fn should_wrap(&self, _hop: &Hop, _context: &ResolutionContext) -> bool {
        true
    }
}

/// Always creates execution boundaries. For resolution-level testing only in Phase A.
pub struct AlwaysSendKeys;

impl CombineStrategy for AlwaysSendKeys {
    fn should_wrap(&self, _hop: &Hop, _context: &ResolutionContext) -> bool {
        false
    }
}
