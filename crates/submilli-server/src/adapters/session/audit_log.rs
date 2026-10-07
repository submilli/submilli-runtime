use crate::application::sessions::ports::{AuditLog, SessionEvent};

pub(crate) struct SessionAuditLog<'a>(pub Option<&'a crate::audit::AuditLog>);

impl AuditLog for SessionAuditLog<'_> {
    fn record(&self, id: &str, event: SessionEvent) {
        let Some(log) = self.0 else {
            return;
        };
        let (event, details) = match event {
            SessionEvent::Created {
                blueprint,
                variables,
            } => (
                "created",
                serde_json::json!({"blueprint": blueprint, "vars": crate::audit::bindings(&variables)}),
            ),
            SessionEvent::Rebound {
                blueprint,
                variables,
            } => (
                "rebound",
                serde_json::json!({"blueprint": blueprint, "vars": crate::audit::bindings(&variables)}),
            ),
            SessionEvent::Found { blueprint } => {
                ("found", serde_json::json!({"blueprint": blueprint}))
            }
            SessionEvent::BlueprintRemoved { blueprint } => (
                "evicted",
                serde_json::json!({"blueprint": blueprint, "reason": "blueprint_deleted"}),
            ),
            SessionEvent::Expired => ("expired", serde_json::json!({"reason": "idle_timeout"})),
            SessionEvent::CredentialsReplaced => ("rebound", serde_json::json!({})),
            SessionEvent::Deleted => ("deleted", serde_json::json!({})),
        };
        let mut fields = details.as_object().cloned().unwrap_or_default();
        fields.insert("session_id".into(), serde_json::json!(id));
        fields.insert("event".into(), serde_json::json!(event));
        fields.insert(
            "principal".into(),
            serde_json::json!(crate::audit::principal()),
        );
        log.emit("session", fields);
    }
}
