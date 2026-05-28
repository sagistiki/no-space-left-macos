//! Core data model: the sized file tree produced by a scan.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// What kind of filesystem entry a [`Node`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
}

/// One node in the scanned tree.
///
/// Only the component `name` is stored (not the full path) to keep large trees
/// memory-light; full paths are reconstructed during traversal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    /// Aggregate logical size in bytes. For directories this is the sum of all
    /// descendants; for files it is the file's own length.
    pub size: u64,
    pub kind: EntryKind,
    pub modified: Option<SystemTime>,
    /// Children. Empty for files. Sorted largest-first after a scan.
    pub children: Vec<Node>,
}

impl Node {
    /// Remove the child at `index` within the descendant reached by following
    /// `nav` (a path of child indices) from this node, subtracting the removed
    /// size from this node and every node along the path so totals stay correct.
    ///
    /// Returns the removed size, or `None` if the path or index is invalid.
    pub fn remove_descendant(&mut self, nav: &[usize], index: usize) -> Option<u64> {
        match nav.split_first() {
            None => {
                if index < self.children.len() {
                    let removed = self.children.remove(index).size;
                    self.size = self.size.saturating_sub(removed);
                    Some(removed)
                } else {
                    None
                }
            }
            Some((&first, rest)) => {
                let child = self.children.get_mut(first)?;
                let removed = child.remove_descendant(rest, index)?;
                self.size = self.size.saturating_sub(removed);
                Some(removed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> Node {
        Node {
            name: name.into(),
            size,
            kind: EntryKind::File,
            modified: None,
            children: vec![],
        }
    }

    fn dir(name: &str, children: Vec<Node>) -> Node {
        let size = children.iter().map(|c| c.size).sum();
        Node {
            name: name.into(),
            size,
            kind: EntryKind::Dir,
            modified: None,
            children,
        }
    }

    #[test]
    fn remove_descendant_top_level_adjusts_root_size() {
        let mut root = dir("root", vec![file("a", 10), file("b", 20)]);

        let removed = root.remove_descendant(&[], 0);

        assert_eq!(removed, Some(10));
        assert_eq!(root.size, 20);
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name, "b");
    }

    #[test]
    fn remove_descendant_nested_adjusts_all_ancestors() {
        let mut root = dir(
            "root",
            vec![file("a", 10), dir("sub", vec![file("b", 20)])],
        );

        let removed = root.remove_descendant(&[1], 0);

        assert_eq!(removed, Some(20));
        assert_eq!(root.size, 10, "root drops by 20");
        assert_eq!(root.children[1].size, 0, "sub drops by 20");
        assert!(root.children[1].children.is_empty());
    }

    #[test]
    fn remove_descendant_invalid_index_returns_none() {
        let mut root = dir("root", vec![file("a", 10)]);
        assert_eq!(root.remove_descendant(&[], 99), None);
        assert_eq!(root.size, 10, "nothing changed");
    }
}
