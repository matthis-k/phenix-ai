//! Deterministic ordering for a caller's semantic dependency projection.
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn order(
    predecessors: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<String>, Vec<String>> {
    fn visit(
        id: &str,
        predecessors: &BTreeMap<String, BTreeSet<String>>,
        done: &mut BTreeSet<String>,
        path: &mut Vec<String>,
        result: &mut Vec<String>,
    ) -> Result<(), Vec<String>> {
        if let Some(at) = path.iter().position(|node| node == id) {
            let mut cycle = path[at..].to_vec();
            cycle.push(id.into());
            return Err(cycle);
        }
        if done.contains(id) {
            return Ok(());
        }
        path.push(id.into());
        for dependency in &predecessors[id] {
            visit(dependency, predecessors, done, path, result)?;
        }
        path.pop();
        done.insert(id.into());
        result.push(id.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    let mut result = Vec::new();
    for node in predecessors.keys() {
        visit(node, predecessors, &mut done, &mut Vec::new(), &mut result)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        edges
            .iter()
            .map(|(node, dependencies)| {
                (
                    (*node).into(),
                    dependencies.iter().map(|s| (*s).into()).collect(),
                )
            })
            .collect()
    }
    #[test]
    fn multi_root_join_and_disconnected_dependencies_preserve_partial_order() {
        let joined = order(&graph(&[("c", &["b", "a"]), ("b", &[]), ("a", &[])])).unwrap();
        assert_eq!(joined, ["a", "b", "c"]);
        let separate = graph(&[("a", &[]), ("b", &[]), ("c", &["a"]), ("d", &["b"])]);
        // An active projection includes only active predecessors. B therefore
        // cannot block A -> C, while a join with both active keeps both edges.
        let active = graph(&[("a", &[]), ("c", &["a"])]);
        assert_eq!(order(&active).unwrap(), ["a", "c"]);
        let reversed = graph(&[("d", &["b"]), ("c", &["a"]), ("b", &[]), ("a", &[])]);
        assert_eq!(order(&separate), order(&reversed));
    }
    #[test]
    fn cycle_diagnostics_are_deterministic() {
        let cyclic = graph(&[("c", &["a"]), ("b", &["c"]), ("a", &["b"])]);
        assert_eq!(
            order(&cyclic),
            Err(vec!["a".into(), "b".into(), "c".into(), "a".into()])
        );
        assert_eq!(
            order(&graph(&[("a", &["a"])])),
            Err(vec!["a".into(), "a".into()])
        );
    }
}
