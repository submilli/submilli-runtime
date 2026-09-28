//! Checked storage operations shared by the parsed and typed AST arenas.
//!
//! Errors carry storage metadata only. A compiler phase adds its stage without
//! reading a potentially invalid node or using that node's source span.
//!
//! IDs remain arena-local indices, not ownership tokens. These operations check
//! storage bounds and capacity; they do not validate a node's child IDs or spans.
//! The maximum node count is `u32::MAX`, matching the legacy arena allocation
//! invariant and leaving `u32::MAX` itself outside the allocated ID range.

use std::{collections::TryReserveError, ops::Range};

use crate::compiler_error::{CompilerFailure, CompilerStage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaKind {
    Expressions,
    Statements,
    TypedExpressions,
    TypedStatements,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaOperation {
    Read,
    Mutate,
    Allocate,
    Iterate,
}

#[derive(Debug)]
pub enum ArenaError {
    InvalidId {
        arena: ArenaKind,
        operation: ArenaOperation,
        id: u32,
        len: usize,
    },
    Capacity {
        arena: ArenaKind,
        operation: ArenaOperation,
        len: usize,
        max_nodes: u32,
    },
    Allocation {
        arena: ArenaKind,
        len: usize,
        source: TryReserveError,
    },
}

impl ArenaError {
    pub fn into_compiler_failure(self, stage: CompilerStage) -> CompilerFailure {
        let message = self.to_string();
        match self {
            Self::InvalidId { .. } => CompilerFailure::Internal {
                stage,
                span: None,
                message,
            },
            Self::Capacity { .. } | Self::Allocation { .. } => CompilerFailure::Limit {
                stage,
                span: None,
                message,
                help: vec!["reduce the size of the program or generated compiler work".into()],
            },
        }
    }
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidId {
                arena,
                operation,
                id,
                len,
            } => {
                write!(
                    f,
                    "{arena:?} {operation:?}: invalid node ID {id} (length {len})"
                )
            }
            Self::Capacity {
                arena,
                operation,
                len,
                max_nodes,
            } => {
                write!(
                    f,
                    "{arena:?} {operation:?}: node capacity exceeded (length {len}, maximum {max_nodes})"
                )
            }
            Self::Allocation { arena, len, source } => {
                write!(
                    f,
                    "{arena:?} Allocate: cannot reserve node storage (length {len}): {source}"
                )
            }
        }
    }
}

impl std::error::Error for ArenaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Allocation { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn get<T>(nodes: &[T], id: u32, arena: ArenaKind) -> Result<&T, ArenaError> {
    usize::try_from(id)
        .ok()
        .and_then(|index| nodes.get(index))
        .ok_or(ArenaError::InvalidId {
            arena,
            operation: ArenaOperation::Read,
            id,
            len: nodes.len(),
        })
}

pub(crate) fn get_mut<T>(nodes: &mut [T], id: u32, arena: ArenaKind) -> Result<&mut T, ArenaError> {
    let len = nodes.len();
    usize::try_from(id)
        .ok()
        .and_then(|index| nodes.get_mut(index))
        .ok_or(ArenaError::InvalidId {
            arena,
            operation: ArenaOperation::Mutate,
            id,
            len,
        })
}

pub(crate) fn push<T>(nodes: &mut Vec<T>, node: T, arena: ArenaKind) -> Result<u32, ArenaError> {
    push_with_limit(nodes, node, arena, u32::MAX)
}

pub(crate) fn ids(len: usize, arena: ArenaKind) -> Result<Range<u32>, ArenaError> {
    let end = u32::try_from(len).map_err(|_| ArenaError::Capacity {
        arena,
        operation: ArenaOperation::Iterate,
        len,
        max_nodes: u32::MAX,
    })?;
    Ok(0..end)
}

fn push_with_limit<T>(
    nodes: &mut Vec<T>,
    node: T,
    arena: ArenaKind,
    max_nodes: u32,
) -> Result<u32, ArenaError> {
    let id = next_id(nodes.len(), arena, max_nodes)?;
    reserve(nodes, 1, arena)?;
    nodes.push(node);
    Ok(id)
}

fn next_id(len: usize, arena: ArenaKind, max_nodes: u32) -> Result<u32, ArenaError> {
    u32::try_from(len)
        .ok()
        .filter(|&id| id < max_nodes)
        .ok_or(ArenaError::Capacity {
            arena,
            operation: ArenaOperation::Allocate,
            len,
            max_nodes,
        })
}

fn reserve<T>(nodes: &mut Vec<T>, additional: usize, arena: ArenaKind) -> Result<(), ArenaError> {
    nodes
        .try_reserve(additional)
        .map_err(|source| ArenaError::Allocation {
            arena,
            len: nodes.len(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_arena_capacity_failure_preserves_nodes_and_allows_reads() {
        let arena = ArenaKind::Expressions;
        let mut nodes = Vec::new();
        assert_eq!(push_with_limit(&mut nodes, 11, arena, 2).unwrap(), 0);
        assert_eq!(push_with_limit(&mut nodes, 22, arena, 2).unwrap(), 1);
        let capacity = nodes.capacity();
        assert!(matches!(
            push_with_limit(&mut nodes, 33, arena, 2),
            Err(ArenaError::Capacity {
                len: 2,
                max_nodes: 2,
                ..
            })
        ));
        assert_eq!(nodes, [11, 22]);
        assert_eq!(nodes.capacity(), capacity);
        assert_eq!(*get(&nodes, 1, arena).unwrap(), 22);
        *get_mut(&mut nodes, 0, arena).unwrap() = 44;
        assert_eq!(nodes, [44, 22]);
        assert!(push_with_limit(&mut Vec::new(), 1, arena, 0).is_err());
    }

    #[test]
    fn checked_arena_id_boundaries_do_not_need_large_allocations() {
        let arena = ArenaKind::TypedStatements;
        assert_eq!(
            next_id(u32::MAX as usize - 1, arena, u32::MAX).unwrap(),
            u32::MAX - 1
        );
        assert!(next_id(u32::MAX as usize, arena, u32::MAX).is_err());
        assert_eq!(ids(0, arena).unwrap(), 0..0);
        assert_eq!(
            ids(u32::MAX as usize, arena).unwrap().next_back(),
            Some(u32::MAX - 1)
        );
        if let Some(overflow) = (u32::MAX as usize).checked_add(1) {
            assert!(next_id(overflow, arena, u32::MAX).is_err());
            assert!(ids(overflow, arena).is_err());
        }
    }

    #[test]
    fn checked_arena_reservation_failure_is_a_limit_without_mutation() {
        let mut nodes = vec![7_u8];
        let error = reserve(&mut nodes, usize::MAX, ArenaKind::Statements).unwrap_err();
        assert!(matches!(&error, ArenaError::Allocation { len: 1, .. }));
        assert!(std::error::Error::source(&error).is_some());
        assert!(matches!(
            error.into_compiler_failure(CompilerStage::Parse),
            CompilerFailure::Limit {
                stage: CompilerStage::Parse,
                span: None,
                ..
            }
        ));
        assert_eq!(nodes, [7]);
        assert_eq!(push(&mut nodes, 8, ArenaKind::Statements).unwrap(), 1);
    }

    #[test]
    fn checked_arena_failures_map_without_node_or_span_access() {
        let error = get::<u8>(&[], u32::MAX, ArenaKind::TypedExpressions).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("4294967295"));
        assert!(message.contains("length 0"));
        assert!(matches!(
            error.into_compiler_failure(CompilerStage::Codegen),
            CompilerFailure::Internal {
                stage: CompilerStage::Codegen,
                span: None,
                ..
            }
        ));
        let error = next_id(2, ArenaKind::Expressions, 2).unwrap_err();
        assert!(matches!(
            error.into_compiler_failure(CompilerStage::Infer),
            CompilerFailure::Limit {
                stage: CompilerStage::Infer,
                span: None,
                ..
            }
        ));
    }
}
