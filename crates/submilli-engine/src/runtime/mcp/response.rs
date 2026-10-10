use serde_json::Value;

use super::{MCP_MAX_RESPONSE_BYTES, McpCallError};

const MAX_DEPTH: usize = 128;
const MAX_NODES: u64 = 100_000;

/// A JSON tree admitted before it crosses the transport/runtime boundary.
/// Private ownership keeps both guest conversion and recursive destruction bounded.
#[derive(Debug)]
pub struct McpResponse {
    value: Value,
    visited_nodes: u64,
}

impl McpResponse {
    /// Validate a transport-owned tree, then move it without cloning or encoding.
    /// On rejection the transport retains ownership. Its decoder must enforce
    /// nesting limits while parsing, including for values it never returns.
    pub fn take(value: &mut Value) -> Result<Self, McpCallError> {
        let mut budget = TreeBudget { bytes: 0, nodes: 0 };
        if !budget.visit(value, 0) {
            return Err(McpCallError::ResponseTooLarge);
        }
        Ok(Self {
            value: value.take(),
            visited_nodes: budget.nodes,
        })
    }

    pub(super) fn value(&self) -> &Value {
        &self.value
    }

    pub(super) fn visited_nodes(&self) -> u64 {
        self.visited_nodes
    }
}

// Bound retained tree data independently of wire JSON escaping. Each node/key
// contributes a fixed allowance, and string bytes are added without rescanning.
struct TreeBudget {
    bytes: usize,
    nodes: u64,
}

impl TreeBudget {
    fn visit(&mut self, value: &Value, depth: usize) -> bool {
        if depth >= MAX_DEPTH || !self.node() {
            return false;
        }
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => true,
            Value::String(value) => self.bytes(value.len()),
            Value::Array(values) => values.iter().all(|value| self.visit(value, depth + 1)),
            Value::Object(fields) => fields.iter().all(|(key, value)| {
                self.node() && self.bytes(key.len()) && self.visit(value, depth + 1)
            }),
        }
    }

    fn node(&mut self) -> bool {
        self.nodes = self.nodes.saturating_add(1);
        self.nodes <= MAX_NODES && self.bytes(16)
    }

    fn bytes(&mut self, bytes: usize) -> bool {
        self.bytes = self.bytes.saturating_add(bytes);
        self.bytes <= MCP_MAX_RESPONSE_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_moves_the_tree_and_counts_payload_and_nodes() {
        let mut value = serde_json::json!({ "x": ["a\nb", null] });
        let mut budget = TreeBudget { bytes: 0, nodes: 0 };
        assert!(budget.visit(&value, 0));
        assert_eq!(budget.bytes, 5 * 16 + 1 + 3);
        assert_eq!(budget.nodes, 5);
        let response = McpResponse::take(&mut value).unwrap();
        assert!(value.is_null());
        assert_eq!(response.value()["x"][0], "a\nb");
    }

    #[test]
    fn admission_rejects_depth_and_node_limits_without_taking_ownership() {
        let mut nested = Value::Null;
        for _ in 0..MAX_DEPTH {
            nested = Value::Array(vec![nested]);
        }
        assert!(matches!(
            McpResponse::take(&mut nested),
            Err(McpCallError::ResponseTooLarge)
        ));
        assert!(nested.is_array());
        let mut wide = Value::Array(vec![Value::Null; MAX_NODES as usize]);
        assert!(McpResponse::take(&mut wide).is_err());
    }
}
