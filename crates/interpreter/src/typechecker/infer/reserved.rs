pub(super) fn is_reserved_object_field(_name: &str) -> bool {
    false
}

pub(super) fn reserved_field_message(name: &str) -> String {
    format!("field name `{name}` is reserved; per-type method override lands with SUB-133")
}

pub(super) fn override_field_signature(name: &str) -> Option<crate::Type> {
    match name {
        "toString" | "toJson" => Some(crate::Type::Function {
            params: Vec::new(),
            ret: Box::new(crate::Type::String),
            predicate: None,
            has_rest: false,
            optional: 0,
        }),
        _ => None,
    }
}
