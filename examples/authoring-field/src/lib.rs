#[phenix_sdk::plugin(id = "test.authored-field", version = "1.0.0", profile = "state")]
mod field {
    #[derive(Default)]
    struct State {
        #[get(id = "notes")]
        notes: Vec<String>,
        local: usize,
    }
    #[start]
    fn start(state: &mut State) -> Result<(), phenix_sdk::Failure> {
        state.notes.reference()?;
        state.local = 1;
        Ok(())
    }
    #[stop]
    fn stop(state: &mut State) -> Result<(), phenix_sdk::Failure> {
        assert_eq!(state.local, 1);
        Ok(())
    }
}
