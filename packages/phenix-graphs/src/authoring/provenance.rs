use super::*;

impl GraphBuilder {
    /// Attribute declarations created or replaced by this callback to one
    /// opaque host origin. Existing units are never relabelled by this scope.
    /// The previous scope is restored on success, error and unwinding.
    pub fn with_origin<R>(
        &mut self,
        origin: &GraphOrigin,
        compose: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.active_origin.replace(origin.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compose(self)));
        self.active_origin = previous;
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    pub(super) fn record_origin(&mut self, declaration: u64) {
        if let Some(origin) = &self.active_origin {
            self.composition_origins.insert(declaration, origin.clone());
        }
    }
}
