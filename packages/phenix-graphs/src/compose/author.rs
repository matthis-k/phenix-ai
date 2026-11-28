//! Typed symbolic patch authoring. These declarations are not compiled handles
//! or grants. The canonical composer checks the completed topology and authority.

use super::{Edit, NetworkPatch};
use crate::{ComponentDef, InputKey, Interface, NetworkError, OutputKey, PortPath, TypeIdentity};
use std::{marker::PhantomData, num::NonZeroUsize, sync::Arc};

/// A symbolic placement with the interface the author expects.
pub struct PatchComponent {
    path: String,
    interface: Arc<Interface>,
}

pub struct PatchInput<T> {
    path: PortPath,
    value: TypeIdentity,
    marker: PhantomData<fn(T)>,
}

pub struct PatchOutput<T> {
    path: PortPath,
    value: TypeIdentity,
    marker: PhantomData<fn() -> T>,
}

impl PatchComponent {
    pub fn at(path: impl Into<String>, interface: Arc<Interface>) -> Self {
        Self {
            path: path.into(),
            interface,
        }
    }

    pub fn input<T: 'static>(&self, key: InputKey<T>) -> Result<PatchInput<T>, NetworkError> {
        let path = PortPath::new(&self.path, key.name());
        let actual = self
            .interface
            .input(key.name())
            .ok_or_else(|| NetworkError::UnknownInput(path.clone()))?;
        let expected = key.definition()?.value;
        if actual.value != expected {
            return Err(NetworkError::WrongType {
                path,
                expected,
                actual: actual.value.clone(),
            });
        }
        Ok(PatchInput {
            path,
            value: expected,
            marker: PhantomData,
        })
    }

    pub fn output<T: 'static>(&self, key: OutputKey<T>) -> Result<PatchOutput<T>, NetworkError> {
        let path = PortPath::new(&self.path, key.name());
        let actual = self
            .interface
            .output(key.name())
            .ok_or_else(|| NetworkError::UnknownOutput(path.clone()))?;
        let expected = key.definition()?.value;
        if actual.value != expected {
            return Err(NetworkError::WrongType {
                path,
                expected,
                actual: actual.value.clone(),
            });
        }
        Ok(PatchOutput {
            path,
            value: expected,
            marker: PhantomData,
        })
    }
}

/// Typed ports derived from the ordinary closure component.
pub struct PatchMap<A, B> {
    pub input: PatchInput<A>,
    pub output: PatchOutput<B>,
}

/// Collects ordinary edits with an explicit, overridable queue capacity.
/// No candidate or authority is retained by the builder.
pub struct PatchBuilder {
    edits: Vec<Edit>,
    capacity: NonZeroUsize,
}

impl PatchBuilder {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            edits: Vec::new(),
            capacity,
        }
    }

    pub fn add(
        &mut self,
        path: impl Into<String>,
        definition: impl Into<Arc<ComponentDef>>,
    ) -> PatchComponent {
        let definition = definition.into();
        let component = PatchComponent::at(path, definition.interface().clone());
        self.edits.push(Edit::add(&component.path, definition));
        component
    }

    pub fn map<A, B>(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(A) -> B + Send + Sync + 'static,
    ) -> PatchMap<A, B>
    where
        A: Clone + Send + Sync + 'static,
        B: Send + Sync + 'static,
    {
        let component = self.add(path, ComponentDef::map(handler));
        PatchMap {
            input: component.input(InputKey::new("input")).expect("map input"),
            output: component.output(OutputKey::new("output")).expect("map output"),
        }
    }

    pub fn connect<T>(
        &mut self,
        from: &PatchOutput<T>,
        to: &PatchInput<T>,
    ) -> Result<(), NetworkError> {
        self.connect_with_capacity(from, to, self.capacity)
    }

    pub fn connect_with_capacity<T>(
        &mut self,
        from: &PatchOutput<T>,
        to: &PatchInput<T>,
        capacity: NonZeroUsize,
    ) -> Result<(), NetworkError> {
        if from.value != to.value {
            return Err(NetworkError::WrongType {
                path: to.path.clone(),
                expected: to.value.clone(),
                actual: from.value.clone(),
            });
        }
        self.edits
            .push(Edit::connect(from.path.clone(), to.path.clone(), capacity));
        Ok(())
    }

    pub fn expose_input<T>(&mut self, name: impl Into<String>, to: &PatchInput<T>) {
        self.edits
            .push(Edit::expose_input(name, [to.path.clone()], self.capacity));
    }

    pub fn expose_output<T>(&mut self, name: impl Into<String>, from: &PatchOutput<T>) {
        self.edits
            .push(Edit::expose_output(name, from.path.clone()));
    }

    /// Complex edits still use the same canonical edit vocabulary.
    pub fn push(&mut self, edit: Edit) {
        self.edits.push(edit);
    }

    pub fn finish(self) -> NetworkPatch {
        NetworkPatch::new(self.edits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_map_authoring_lowers_to_exact_symbolic_edits() {
        let capacity = NonZeroUsize::new(2).unwrap();
        let mut patch = PatchBuilder::new(capacity);
        let a = patch.map("a", |n: i64| n + 1);
        let b = patch.map("b", |n: i64| n * 2);
        patch.connect(&a.output, &b.input).unwrap();
        patch.expose_input("request", &a.input);
        patch.expose_output("reply", &b.output);
        let patch = patch.finish();
        assert_eq!(patch.edits().len(), 5);
        assert_eq!(
            patch.footprints(),
            NetworkPatch::new([
                Edit::add("a", ComponentDef::map(|n: i64| n + 1)),
                Edit::add("b", ComponentDef::map(|n: i64| n * 2)),
                Edit::connect(("a", "output"), ("b", "input"), capacity),
                Edit::expose_input("request", [("a", "input")], capacity),
                Edit::expose_output("reply", ("b", "output")),
            ])
            .footprints()
        );
    }

    #[test]
    fn wrong_port_mode_is_rejected_before_an_edit_is_added() {
        let mut patch = PatchBuilder::new(NonZeroUsize::new(1).unwrap());
        let component = patch.add("a", ComponentDef::map(|n: i64| n));
        assert!(component.input(InputKey::<String>::new("input")).is_err());
        assert!(component.input(InputKey::<i64>::encoded("input")).is_err());
        assert_eq!(patch.finish().edits().len(), 1);
    }
}
