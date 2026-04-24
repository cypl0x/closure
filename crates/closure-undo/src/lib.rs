//! Branching undo-tree.
//!
//! An [`UndoTree`] is a rooted DAG of edit nodes. Each node stores a
//! payload plus a parent pointer. After an `undo()`, a fresh `apply()`
//! creates a new branch rather than overwriting redo history, so no
//! user action is ever lost.
//!
//! The payload is generic so this crate can sit under
//! [`closure_core`] without a circular dependency — the kernel wraps
//! `UndoTree<Edit>` to connect it to `Document`.

#![forbid(unsafe_code)]

use thiserror::Error;
use ulid::Ulid;

/// Stable identifier for a node in the undo tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u128);

/// A single node in the tree.
#[derive(Debug, Clone)]
pub struct UndoNode<T> {
    /// Stable id (ULID-backed).
    pub id: NodeId,
    /// Parent node id; `None` for the root.
    pub parent: Option<NodeId>,
    /// Child node ids in branch-creation order.
    pub children: Vec<NodeId>,
    /// User payload (typically an `Edit`).
    pub payload: T,
}

/// Undo-tree errors.
#[derive(Debug, Error)]
pub enum UndoError {
    /// The requested node does not exist.
    #[error("no such undo-tree node")]
    NotFound,
    /// Operation attempted at the root where no further undo is possible.
    #[error("at root")]
    AtRoot,
}

/// Branching undo tree.
#[derive(Debug, Clone)]
pub struct UndoTree<T> {
    nodes: Vec<UndoNode<T>>,
    current: Option<NodeId>,
}

impl<T> Default for UndoTree<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> UndoTree<T> {
    /// A fresh tree with no edits.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            current: None,
        }
    }

    /// Total number of nodes in the tree (including the implicit root
    /// position before any edits).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether no edits have been applied.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The id of the currently-applied edit, if any.
    #[must_use]
    pub const fn current(&self) -> Option<NodeId> {
        self.current
    }

    /// Apply a new edit. If we are currently on node X, the new node
    /// becomes X's child (and the new `current`). If we are at the
    /// root (no edits yet), the new node becomes a fresh root-level
    /// node.
    pub fn apply(&mut self, payload: T) -> NodeId {
        let id = NodeId(Ulid::new().into());
        let parent = self.current;
        self.nodes.push(UndoNode {
            id,
            parent,
            children: Vec::new(),
            payload,
        });
        if let Some(p) = parent
            && let Some(idx) = self.index_of(p)
        {
            self.nodes[idx].children.push(id);
        }
        self.current = Some(id);
        id
    }

    /// Move one step toward the root, if possible.
    pub fn undo(&mut self) -> Result<Option<NodeId>, UndoError> {
        match self.current {
            None => Err(UndoError::AtRoot),
            Some(id) => {
                let parent = self.node(id).and_then(|n| n.parent);
                self.current = parent;
                Ok(parent)
            }
        }
    }

    /// Move to a named child of the current node. Passing `None`
    /// selects the most recently-created child (the canonical redo).
    pub fn redo(&mut self, branch: Option<NodeId>) -> Result<NodeId, UndoError> {
        let children: Vec<NodeId> = match self.current {
            None => self
                .nodes
                .iter()
                .filter(|n| n.parent.is_none())
                .map(|n| n.id)
                .collect(),
            Some(id) => self
                .node(id)
                .map(|n| n.children.clone())
                .unwrap_or_default(),
        };
        if children.is_empty() {
            return Err(UndoError::NotFound);
        }
        let next = match branch {
            Some(id) => {
                if !children.contains(&id) {
                    return Err(UndoError::NotFound);
                }
                id
            }
            None => *children.last().unwrap_or(&children[0]),
        };
        self.current = Some(next);
        Ok(next)
    }

    /// Lookup a node by id.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&UndoNode<T>> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn index_of(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// All nodes in insertion order.
    #[must_use]
    pub fn nodes(&self) -> &[UndoNode<T>] {
        &self.nodes
    }
}
