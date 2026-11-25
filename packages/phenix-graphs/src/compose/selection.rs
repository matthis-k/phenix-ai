//! Exact signed-conjunction equations, with O(V + E) live solver storage.
//!
//! Every equation is `x = AND(dependencies, NOT blockers)`. An assignment updates
//! every outgoing counter before propagation may report a contradiction.
//! Backtracking only undoes fully updated assignments; budget failure may abandon
//! a partially mutated private attempt, but logical backtracking never does.

use super::{ComposeError, Registration, SelectionLimits};
use std::collections::{BTreeMap, VecDeque};

pub(super) fn select(
    registrations: &BTreeMap<String, Registration>,
    order: &[String],
    blockers: &[(usize, usize)],
    limits: SelectionLimits,
) -> Result<Vec<bool>, ComposeError> {
    let mut solver = Solver::new(order.len(), limits)?;
    let mut indices = BTreeMap::new();
    for (index, id) in order.iter().enumerate() {
        solver.budget.charge()?;
        indices.insert(id.as_str(), index);
    }
    for (target, id) in order.iter().enumerate() {
        for dependency in &registrations[id].dependencies {
            solver.budget.charge()?;
            solver.add_edge(indices[dependency.as_str()], target, true)?;
        }
    }
    for &(source, target) in blockers {
        solver.add_edge(source, target, false)?;
    }
    solver.solve()
}

struct Budget {
    limit: usize,
    remaining: usize,
}
impl Budget {
    fn charge(&mut self) -> Result<(), ComposeError> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| self.exceeded())?;
        Ok(())
    }

    fn exceeded(&self) -> ComposeError {
        ComposeError::SelectionBudgetExceeded { limit: self.limit }
    }
}

struct Equation {
    value: Option<bool>,
    unknown_count: usize,
    false_count: usize,
    outgoing: Vec<(usize, bool)>,
}

struct Frame {
    checkpoint: usize,
    variable: usize,
    tried_true: bool,
}

struct Solver {
    equations: Vec<Equation>,
    queue: VecDeque<usize>,
    trail: Vec<usize>,
    budget: Budget,
}

impl Solver {
    fn new(size: usize, limits: SelectionLimits) -> Result<Self, ComposeError> {
        let mut solver = Self {
            equations: Vec::new(),
            queue: VecDeque::new(),
            trail: Vec::new(),
            budget: Budget {
                limit: limits.max_work,
                remaining: limits.max_work,
            },
        };
        for _ in 0..size {
            solver.budget.charge()?;
            solver.equations.push(Equation {
                value: None,
                unknown_count: 0,
                false_count: 0,
                outgoing: Vec::new(),
            });
        }
        Ok(solver)
    }

    fn add_edge(
        &mut self,
        source: usize,
        target: usize,
        positive: bool,
    ) -> Result<(), ComposeError> {
        self.budget.charge()?;
        let count = &mut self.equations[target].unknown_count;
        *count = count.checked_add(1).ok_or_else(|| self.budget.exceeded())?;
        self.equations[source].outgoing.push((target, positive));
        Ok(())
    }

    fn enqueue(&mut self, equation: usize) -> Result<(), ComposeError> {
        self.budget.charge()?;
        self.queue.push_back(equation);
        Ok(())
    }

    fn assign(&mut self, variable: usize, value: bool) -> Result<(), ComposeError> {
        self.budget.charge()?;
        debug_assert!(self.equations[variable].value.is_none());
        self.equations[variable].value = Some(value);
        self.trail.push(variable);
        // A decision's own RHS may already be known. Revisit it as well as users.
        self.enqueue(variable)?;
        for edge in 0..self.equations[variable].outgoing.len() {
            self.budget.charge()?;
            let (target, positive) = self.equations[variable].outgoing[edge];
            let equation = &mut self.equations[target];
            equation.unknown_count = equation
                .unknown_count
                .checked_sub(1)
                .ok_or_else(|| self.budget.exceeded())?;
            if value != positive {
                equation.false_count = equation
                    .false_count
                    .checked_add(1)
                    .ok_or_else(|| self.budget.exceeded())?;
            }
            self.enqueue(target)?;
        }
        Ok(())
    }

    fn propagate(&mut self) -> Result<bool, ComposeError> {
        while !self.queue.is_empty() {
            self.budget.charge()?;
            let variable = self.queue.pop_front().unwrap();
            let equation = &self.equations[variable];
            let rhs = if equation.false_count != 0 {
                false
            } else if equation.unknown_count == 0 {
                true
            } else {
                continue;
            };
            match equation.value {
                Some(value) if value != rhs => return Ok(false),
                None => self.assign(variable, rhs)?,
                _ => {}
            }
        }
        Ok(true)
    }

    fn rollback(&mut self, checkpoint: usize) -> Result<(), ComposeError> {
        // Entries from the abandoned branch must not leak into its sibling.
        while !self.queue.is_empty() {
            self.budget.charge()?;
            self.queue.pop_front();
        }
        while self.trail.len() > checkpoint {
            self.budget.charge()?;
            let variable = self.trail.pop().unwrap();
            let value = self.equations[variable].value.take().unwrap();
            for edge in 0..self.equations[variable].outgoing.len() {
                self.budget.charge()?;
                let (target, positive) = self.equations[variable].outgoing[edge];
                let equation = &mut self.equations[target];
                equation.unknown_count = equation
                    .unknown_count
                    .checked_add(1)
                    .ok_or_else(|| self.budget.exceeded())?;
                if value != positive {
                    equation.false_count = equation
                        .false_count
                        .checked_sub(1)
                        .ok_or_else(|| self.budget.exceeded())?;
                }
            }
        }
        Ok(())
    }

    fn solve(mut self) -> Result<Vec<bool>, ComposeError> {
        for variable in 0..self.equations.len() {
            self.budget.charge()?;
            if self.equations[variable].unknown_count == 0 {
                self.enqueue(variable)?;
            }
        }
        let mut frames: Vec<Frame> = Vec::new();
        let mut first_model = None;
        loop {
            if self.propagate()? {
                let mut unknown = None;
                for (variable, equation) in self.equations.iter().enumerate() {
                    self.budget.charge()?;
                    if equation.value.is_none() {
                        unknown = Some(variable);
                        break;
                    }
                }
                if let Some(variable) = unknown {
                    self.budget.charge()?;
                    frames.push(Frame {
                        checkpoint: self.trail.len(),
                        variable,
                        tried_true: false,
                    });
                    self.assign(variable, false)?;
                    continue;
                }
                // Propagation has drained completely, so this is a fixed point,
                // not merely a complete assignment waiting to contradict itself.
                if first_model.is_some() {
                    return Err(ComposeError::AmbiguousSelection);
                }
                let mut model = Vec::new();
                for equation in &self.equations {
                    self.budget.charge()?;
                    model.push(equation.value.unwrap());
                }
                first_model = Some(model);
            }

            loop {
                let Some(frame) = frames.last_mut() else {
                    // Conclusive results need no cleanup of private branch state.
                    return first_model.ok_or(ComposeError::NoConsistentSelection);
                };
                self.budget.charge()?;
                if frame.tried_true {
                    frames.pop();
                    continue;
                }
                frame.tried_true = true;
                self.rollback(frame.checkpoint)?;
                self.assign(frame.variable, true)?;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solver(size: usize, edges: &[(usize, usize, bool)], limit: usize) -> Solver {
        let mut solver = Solver::new(size, SelectionLimits { max_work: limit }).unwrap();
        for &(source, target, positive) in edges {
            solver.add_edge(source, target, positive).unwrap();
        }
        solver
    }

    #[test]
    fn mixed_literals_and_multiple_blockers_propagate() {
        let edges = [
            (0, 1, false),
            (0, 2, true),
            (2, 3, true),
            (0, 3, false),
            (1, 3, false),
            (2, 4, true),
            (1, 4, false),
            (3, 4, false),
            (1, 5, true),
            (3, 5, false),
        ];
        assert_eq!(
            solver(6, &edges, 1_000).solve(),
            Ok(vec![true, false, true, false, true, false])
        );
    }

    #[test]
    fn contradiction_updates_all_edges_before_rollback_restores_counters() {
        let edges = [
            (0, 0, false),
            (0, 1, true),
            (0, 2, false),
            (0, 3, true),
            (1, 3, false),
            (2, 3, true),
        ];
        let mut solver = solver(4, &edges, 1_000);
        for value in [false, true, false] {
            solver.assign(0, value).unwrap();
            assert!(!solver.propagate().unwrap());
            assert!(!solver.queue.is_empty());
            assert_eq!(solver.equations[3].unknown_count, 2);
            assert_eq!(solver.equations[3].false_count, usize::from(!value));
            solver.rollback(0).unwrap();
            assert!(solver.queue.is_empty());
            assert!(solver.trail.is_empty());
            for (equation, count) in solver.equations.iter().zip([1, 1, 1, 3]) {
                assert_eq!(equation.value, None);
                assert_eq!(equation.unknown_count, count);
                assert_eq!(equation.false_count, 0);
            }
        }
    }

    #[test]
    fn unique_cyclic_model_requires_exhausting_the_other_branch() {
        // x = y & z; y = x; z = !x. False is a model, true contradicts x.
        let edges = [(1, 0, true), (2, 0, true), (0, 1, true), (0, 2, false)];
        assert_eq!(
            solver(3, &edges, 1_000).solve(),
            Ok(vec![false, false, true])
        );
        // Find the precise conclusive cost and require failure at every earlier
        // cutoff, including after the first model has already been captured.
        let required = (7..1_000)
            .find(|&limit| solver(3, &edges, limit).solve().is_ok())
            .unwrap();
        for limit in 7..required {
            assert_eq!(
                solver(3, &edges, limit).solve(),
                Err(ComposeError::SelectionBudgetExceeded { limit })
            );
        }
        assert_eq!(
            solver(3, &edges, required).solve(),
            Ok(vec![false, false, true])
        );
    }

    #[test]
    fn exact_budget_conclusive_results_do_not_need_cleanup() {
        assert_eq!(solver(0, &[], 0).solve(), Ok(vec![]));
        assert_eq!(solver(1, &[], 9).solve(), Ok(vec![true]));
        assert_eq!(
            solver(1, &[], 8).solve(),
            Err(ComposeError::SelectionBudgetExceeded { limit: 8 })
        );
        // x = !x ends with a queued contradiction and an assigned private trail.
        assert_eq!(
            solver(1, &[(0, 0, false)], 20).solve(),
            Err(ComposeError::NoConsistentSelection)
        );
        assert_eq!(
            solver(1, &[(0, 0, false)], 19).solve(),
            Err(ComposeError::SelectionBudgetExceeded { limit: 19 })
        );
        assert_eq!(
            solver(1, &[(0, 0, true)], 23).solve(),
            Err(ComposeError::AmbiguousSelection)
        );
        assert_eq!(
            solver(1, &[(0, 0, true)], 22).solve(),
            Err(ComposeError::SelectionBudgetExceeded { limit: 22 })
        );
    }

    #[test]
    fn complete_assignments_are_not_models_until_propagation_drains() {
        // x = !y; y = x. Each branch fills both variables before contradicting x.
        assert_eq!(
            solver(2, &[(1, 0, false), (0, 1, true)], 1_000).solve(),
            Err(ComposeError::NoConsistentSelection)
        );
    }

    #[test]
    fn deep_acyclic_influence_needs_no_recursion_or_search_frames() {
        let size = 20_000;
        let edges: Vec<_> = (1..size).map(|i| (i - 1, i, true)).collect();
        assert_eq!(
            solver(size, &edges, size * 11 - 2).solve(),
            Ok(vec![true; size])
        );
    }

    #[test]
    fn all_three_variable_signed_equations_match_exhaustive_truth_tables() {
        // Independently enumerate absent/positive/negative literals in all nine
        // source/target positions, including self-influence and mixed cycles.
        for mut code in 0..3_usize.pow(9) {
            let original = code;
            let mut edges = Vec::new();
            for target in 0..3 {
                for source in 0..3 {
                    match code % 3 {
                        1 => edges.push((source, target, true)),
                        2 => edges.push((source, target, false)),
                        _ => {}
                    }
                    code /= 3;
                }
            }
            let models: Vec<Vec<bool>> = (0..8)
                .map(|bits| (0..3).map(|i| bits & (1 << i) != 0).collect())
                .filter(|values: &Vec<bool>| {
                    (0..3).all(|target| {
                        values[target]
                            == edges
                                .iter()
                                .filter(|&&(_, to, _)| to == target)
                                .all(|&(source, _, positive)| values[source] == positive)
                    })
                })
                .collect();
            let expected = match models.as_slice() {
                [] => Err(ComposeError::NoConsistentSelection),
                [model] => Ok(model.clone()),
                _ => Err(ComposeError::AmbiguousSelection),
            };
            assert_eq!(
                solver(3, &edges, 10_000).solve(),
                expected,
                "case {original}"
            );
        }
    }
}
