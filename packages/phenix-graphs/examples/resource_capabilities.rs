use phenix_graph::{ProcessContext, ResourceError, ResourceGrant, ValidatedResourceSet};

// Only this application module can issue facades or access the underlying lock.
pub mod application {
    use std::sync::{Arc, RwLock};

    pub struct ReadFacade(Arc<RwLock<i64>>);
    pub struct WriteFacade(Arc<RwLock<i64>>);

    impl ReadFacade {
        pub fn read(&self) -> i64 {
            *self.0.read().unwrap()
        }
    }
    impl WriteFacade {
        pub fn read(&self) -> i64 {
            *self.0.read().unwrap()
        }
        pub fn write(&self, value: i64) {
            *self.0.write().unwrap() = value;
        }
    }
    pub fn provision(initial: i64) -> (ReadFacade, WriteFacade) {
        let state = Arc::new(RwLock::new(initial));
        (ReadFacade(state.clone()), WriteFacade(state))
    }
}

fn main() -> Result<(), ResourceError> {
    use application::{ReadFacade, WriteFacade};

    let (reader, writer) = application::provision(7);
    let read_context =
        ProcessContext::default().with_resources(ValidatedResourceSet::try_new(vec![
            ResourceGrant::new("counter", reader),
        ])?);
    let write_context =
        ProcessContext::default().with_resources(ValidatedResourceSet::try_new(vec![
            ResourceGrant::new("counter", writer),
        ])?);

    assert_eq!(read_context.resource::<ReadFacade>("counter")?.read(), 7);
    assert!(matches!(
        read_context.resource::<WriteFacade>("counter"),
        Err(ResourceError::WrongType("counter"))
    ));
    let writer = write_context.resource::<WriteFacade>("counter")?;
    writer.write(8);
    assert_eq!(writer.read(), 8);
    assert_eq!(read_context.resource::<ReadFacade>("counter")?.read(), 8);
    Ok(())
}
