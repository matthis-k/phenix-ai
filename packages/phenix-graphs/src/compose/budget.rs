//! Composition-only logical work/storage accounting, borrowed for one attempt.
use super::{ComposeError, ConnectionKey, Edit, Footprint, PlanningLimits, Registration};
use crate::network::PortPath;

pub(super) struct PlanningBudget<'a> {
    limit: usize,
    used: &'a mut usize,
    #[cfg(test)]
    pub(super) ancestor_cells: usize,
    #[cfg(test)]
    pub(super) blockers: usize,
    #[cfg(test)]
    pub(super) posting_owners: usize,
}

impl<'a> PlanningBudget<'a> {
    pub(super) fn new(limits: PlanningLimits, used: &'a mut usize) -> Result<Self, ComposeError> {
        let result = Self {
            limit: limits.max_units,
            used,
            #[cfg(test)]
            ancestor_cells: 0,
            #[cfg(test)]
            blockers: 0,
            #[cfg(test)]
            posting_owners: 0,
        };
        if *result.used > result.limit {
            return Err(result.exceeded());
        }
        Ok(result)
    }

    fn exceeded(&self) -> ComposeError {
        ComposeError::PlanningBudgetExceeded { limit: self.limit }
    }

    pub(super) fn charge(&mut self, units: usize) -> Result<(), ComposeError> {
        let next = self
            .used
            .checked_add(units)
            .ok_or_else(|| self.exceeded())?;
        if next > self.limit {
            return Err(self.exceeded());
        }
        *self.used = next;
        Ok(())
    }

    /// One logical operation plus all query/copy/scan bytes. BTree-internal
    /// comparisons and allocator capacity rounding are deliberately excluded.
    pub(super) fn text(&mut self, text: &str) -> Result<(), ComposeError> {
        let units = text.len().checked_add(1).ok_or_else(|| self.exceeded())?;
        self.charge(units)
    }

    pub(super) fn port(&mut self, port: &PortPath) -> Result<(), ComposeError> {
        self.text(&port.process)?;
        self.text(&port.port)
    }

    pub(super) fn edge(&mut self, edge: &ConnectionKey) -> Result<(), ComposeError> {
        self.port(&edge.from)?;
        self.port(&edge.to)
    }

    pub(super) fn footprint(&mut self, footprint: &Footprint) -> Result<(), ComposeError> {
        self.charge(1)?;
        match footprint {
            Footprint::Connection(edge) => self.edge(edge),
            Footprint::NestedInputExport { parent, name }
            | Footprint::NestedOutputExport { parent, name } => {
                self.text(parent)?;
                self.text(name)
            }
            Footprint::Component(path)
            | Footprint::InputExport(path)
            | Footprint::OutputExport(path)
            | Footprint::Source(path) => self.text(path),
        }
    }

    /// Metadata only; Add/Replace definitions are Arc-owned. Their structural
    /// origin traversal is charged separately inside validation.
    pub(super) fn edit(&mut self, edit: &Edit) -> Result<(), ComposeError> {
        self.charge(1)?;
        match edit {
            Edit::Add { path, .. } | Edit::Replace { path, .. } | Edit::AddSource(path) => {
                self.text(path)
            }
            Edit::Connect(edge) => {
                self.port(&edge.from)?;
                self.port(&edge.to)
            }
            Edit::Disconnect(edge) => self.edge(edge),
            Edit::ExposeInput { name, targets, .. } => {
                self.text(name)?;
                for target in targets {
                    self.port(target)?;
                }
                Ok(())
            }
            Edit::ExposeOutput { name, from } => {
                self.text(name)?;
                self.port(from)
            }
            Edit::ExposeNestedInput {
                parent,
                name,
                targets,
                ..
            } => {
                self.text(parent)?;
                self.text(name)?;
                for target in targets {
                    self.port(target)?;
                }
                Ok(())
            }
            Edit::ExposeNestedOutput { parent, name, from } => {
                self.text(parent)?;
                self.text(name)?;
                self.port(from)
            }
            Edit::Splice {
                anchor,
                input,
                output,
            } => {
                self.edge(anchor)?;
                self.port(input)?;
                self.port(output)
            }
        }
    }

    pub(super) fn registration_copy(
        &mut self,
        registration: &Registration,
    ) -> Result<(), ComposeError> {
        self.charge(1)?;
        self.text(&registration.id)?;
        for dependency in &registration.dependencies {
            self.text(dependency)?;
            self.charge(1)?;
        }
        for grant in &registration.grants {
            self.footprint(grant)?;
            self.charge(1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_are_exact_overflow_safe_and_never_refunded() {
        let mut used = 7;
        let mut budget = PlanningBudget::new(PlanningLimits { max_units: 10 }, &mut used).unwrap();
        budget.charge(3).unwrap();
        budget.charge(0).unwrap();
        assert_eq!(
            budget.charge(1),
            Err(ComposeError::PlanningBudgetExceeded { limit: 10 })
        );
        assert_eq!(used, 10);
        assert!(PlanningBudget::new(PlanningLimits { max_units: 9 }, &mut used).is_err());
        let mut used = usize::MAX - 1;
        let mut budget = PlanningBudget::new(
            PlanningLimits {
                max_units: usize::MAX,
            },
            &mut used,
        )
        .unwrap();
        assert!(budget.charge(2).is_err());
        assert_eq!(used, usize::MAX - 1);
    }
}
