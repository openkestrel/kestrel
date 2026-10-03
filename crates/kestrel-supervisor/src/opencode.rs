use agent_client_protocol::JsonRpcNotification;
use serde::{Deserialize, Serialize};

pub const CHILD_SESSION_UPDATES: &str = "opencode/child-session-updates";

/// Taken untyped, because a notification that fails to parse fails on the connection.
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcNotification)]
#[notification(method = "opencode/session/child_update")]
#[serde(transparent)]
pub struct ChildUpdateNotification(pub serde_json::Value);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChildUpdate {
    /// Already projected onto the parent, its tool call ids prefixed by the child's.
    Update {
        update: serde_json::Value,
    },
    Status,
}
