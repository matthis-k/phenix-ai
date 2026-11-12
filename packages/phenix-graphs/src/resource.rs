//! Shared resource storage. Access does not schedule work; processes require
//! explicit `ResourceGrant`s to access this existing authority.
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Default)]
pub struct Store {
    entries: BTreeMap<&'static str, Arc<dyn Any + Send + Sync>>,
}
impl Store {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert<T: Send + Sync + 'static>(&mut self, key: &'static str, value: T) {
        self.entries.insert(key, Arc::new(RwLock::new(value)));
    }
    pub fn get<T: Send + Sync + 'static>(&self, key: &'static str) -> Result<&RwLock<T>, String> {
        self.entries
            .get(key)
            .ok_or_else(|| format!("resource {key} absent"))?
            .as_ref()
            .downcast_ref::<RwLock<T>>()
            .ok_or_else(|| format!("resource {key} has wrong Rust type"))
    }
}
