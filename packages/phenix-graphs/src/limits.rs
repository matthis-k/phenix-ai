//! Safety bounds on recursive network expansion and compiler work.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompileLimits {
    pub max_depth: usize,
    pub max_instances: u64,
    pub max_work: u64,
}
impl Default for CompileLimits {
    fn default() -> Self {
        Self {
            max_depth: 128,
            max_instances: 4096,
            max_work: 1_000_000,
        }
    }
}
