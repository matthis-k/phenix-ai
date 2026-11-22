//! Shared, iterative SCC traversal for immutable topology inspection and
//! actual runtime publication-wait diagnostics. No static progress claim.
//! Callers construct adjacency lists from their own *grounded* edges.
pub(crate) fn cyclic_components(edges: &[Vec<usize>], reverse: &[Vec<usize>]) -> Vec<Vec<usize>> {
    debug_assert_eq!(edges.len(), reverse.len());
    let count = edges.len();
    let mut visited = vec![false; count];
    let mut order = Vec::with_capacity(count);
    for root in 0..count {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut stack = vec![(root, 0)];
        while let Some((node, next)) = stack.last_mut() {
            if *next < edges[*node].len() {
                let child = edges[*node][*next];
                *next += 1;
                if !visited[child] {
                    visited[child] = true;
                    stack.push((child, 0));
                }
            } else {
                let (node, _) = stack.pop().unwrap();
                order.push(node);
            }
        }
    }
    visited.fill(false);
    let mut result = Vec::new();
    for root in order.into_iter().rev() {
        if visited[root] {
            continue;
        }
        let mut members = Vec::new();
        let mut stack = vec![root];
        visited[root] = true;
        while let Some(node) = stack.pop() {
            members.push(node);
            for parent in &reverse[node] {
                if !visited[*parent] {
                    visited[*parent] = true;
                    stack.push(*parent);
                }
            }
        }
        if members.len() == 1 && !edges[root].contains(&root) {
            continue;
        }
        members.sort_unstable();
        result.push(members);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_loops_and_two_disjoint_cycles_are_deterministic() {
        let edges = vec![
            vec![0, 1], vec![2], vec![1, 3],
            vec![4], vec![3], vec![],
        ];
        let mut reverse = vec![vec![]; edges.len()];
        for (from, destinations) in edges.iter().enumerate() {
            for &to in destinations {
                reverse[to].push(from);
            }
        }
        let mut found = cyclic_components(&edges, &reverse);
        found.sort();
        assert_eq!(found, vec![vec![0], vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn deep_acyclic_chain_remains_iterative() {
        let count = 20_000;
        let mut edges = vec![vec![]; count];
        let mut reverse = vec![vec![]; count];
        for index in 1..count {
            edges[index - 1].push(index);
            reverse[index].push(index - 1);
        }
        assert!(cyclic_components(&edges, &reverse).is_empty());
    }
}
