#[phenix_sdk::plugin(id = "test.authored-field", version = "1.0.0", profile = "state")]
mod field {
    #[derive(Default)]
    struct State {
        // The Rust Vec remains authoritative, while Phenix sees a read-only
        // typed value. No Cell<T> replaces this field.
        #[expose]
        notes: Vec<String>,
        local: usize,
    }
    #[start]
    fn start(state: &mut State) -> Result<(), phenix_sdk::Failure> {
        assert!(state.notes.is_empty());
        state.local = 1;
        Ok(())
    }
    #[stop]
    fn stop(state: &mut State) -> Result<(), phenix_sdk::Failure> {
        assert_eq!(state.local, 1);
        Ok(())
    }
}
