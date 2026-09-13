use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::{depth_first_search, Control, Dfs, DfsEvent},
};
use std::collections::{BTreeMap, BTreeSet};

/// Internal mapping glue around petgraph.
///
/// Domain meaning and semantic ordering stay at the call site. This type only
/// centralizes stable node indexing plus reusable traversal mechanics.
pub(crate) struct DirectedGraph<N> {
    graph: DiGraph<N, ()>,
    indices: BTreeMap<N, NodeIndex>,
}

impl<N> DirectedGraph<N>
where
    N: Clone + Ord,
{
    pub(crate) fn from_nodes(nodes: impl IntoIterator<Item = N>) -> Self {
        let mut graph = DiGraph::new();
        let mut indices = BTreeMap::new();
        for node in nodes.into_iter().collect::<BTreeSet<_>>() {
            let index = graph.add_node(node.clone());
            indices.insert(node, index);
        }
        Self { graph, indices }
    }

    pub(crate) fn add_edge(&mut self, from: &N, to: &N) {
        let from = self
            .indices
            .get(from)
            .copied()
            .expect("graph edges are added only from registered nodes");
        let to = self
            .indices
            .get(to)
            .copied()
            .expect("graph edges are added only to registered nodes");
        self.graph.add_edge(from, to, ());
    }

    pub(crate) fn reachable_from(&self, root: &N) -> BTreeSet<N> {
        let Some(&root) = self.indices.get(root) else {
            return BTreeSet::new();
        };
        let mut dfs = Dfs::new(&self.graph, root);
        let mut reachable = BTreeSet::new();
        while let Some(index) = dfs.next(&self.graph) {
            reachable.insert(self.graph[index].clone());
        }
        reachable
    }

    pub(crate) fn cycle_path_from(&self, root: &N) -> Option<Vec<N>> {
        let &root = self.indices.get(root)?;
        let mut path = Vec::new();
        let result = depth_first_search(&self.graph, Some(root), |event| match event {
            DfsEvent::Discover(node, _) => {
                path.push(node);
                Control::Continue
            }
            DfsEvent::BackEdge(_, target) => {
                let position = path
                    .iter()
                    .position(|candidate| *candidate == target)
                    .expect("back edge target must be on the active DFS path");
                let mut cycle = path[position..].to_vec();
                cycle.push(target);
                Control::Break(cycle)
            }
            DfsEvent::Finish(node, _) => {
                debug_assert_eq!(path.pop(), Some(node));
                Control::Continue
            }
            DfsEvent::TreeEdge(_, _) | DfsEvent::CrossForwardEdge(_, _) => Control::Continue,
        });

        match result {
            Control::Break(cycle) => Some(
                cycle
                    .into_iter()
                    .map(|index| self.graph[index].clone())
                    .collect(),
            ),
            Control::Continue | Control::Prune => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_path_keeps_the_concrete_offending_path() {
        let mut graph = DirectedGraph::from_nodes(["third", "first", "second", "unrelated"]);
        graph.add_edge(&"first", &"second");
        graph.add_edge(&"second", &"third");
        graph.add_edge(&"third", &"first");

        assert_eq!(
            graph.cycle_path_from(&"first"),
            Some(vec!["first", "second", "third", "first"])
        );
        assert_eq!(graph.cycle_path_from(&"unrelated"), None);
    }

    #[test]
    fn reachability_is_membership_not_traversal_order() {
        let mut graph = DirectedGraph::from_nodes(["root", "z-child", "a-child", "other"]);
        graph.add_edge(&"root", &"z-child");
        graph.add_edge(&"root", &"a-child");

        assert_eq!(
            graph.reachable_from(&"root"),
            BTreeSet::from(["a-child", "root", "z-child"])
        );
        assert_eq!(graph.reachable_from(&"other"), BTreeSet::from(["other"]));
    }
}
