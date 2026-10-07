use submilli_blueprint::VarBindings;

pub(crate) trait AuditLog: Send + Sync {
    fn record(&self, id: &str, event: SessionEvent);
}

pub(crate) enum SessionEvent {
    Created {
        blueprint: String,
        variables: VarBindings,
    },
    Rebound {
        blueprint: String,
        variables: VarBindings,
    },
    Found {
        blueprint: String,
    },
    BlueprintRemoved {
        blueprint: String,
    },
    Expired,
    CredentialsReplaced,
    Deleted,
}
