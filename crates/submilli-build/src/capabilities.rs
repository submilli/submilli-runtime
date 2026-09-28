//! Derived package capability schema (`capabilities.yaml`).

use std::collections::{BTreeMap, BTreeSet};

use interpreter::{
    DerivedCapability, DocCapabilityBindingKind, DocCapabilityLiteral, PackageDeclaration, Param,
    Type, ValueKind,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySchema {
    pub namespace: String,
    pub provides: Vec<ProvidedCapability>,
    pub requires: Vec<RequiredCapability>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProvidedCapability {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: BTreeMap<String, ProvidedField>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProvidedField {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequiredCapability {
    pub capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

pub fn derive_capability_schema(
    declaration: &PackageDeclaration,
    requires: &[DerivedCapability],
) -> CapabilitySchema {
    let provides = provided_capabilities(declaration);

    let mut seen_requires = BTreeSet::new();
    let mut requires = requires
        .iter()
        .filter_map(|required| {
            let entry = RequiredCapability {
                capability: required.capability.clone(),
                filter: required.filter.clone(),
            };
            seen_requires.insert(entry.clone()).then_some(entry)
        })
        .collect::<Vec<_>>();
    requires.sort();

    CapabilitySchema {
        namespace: namespace_from_package(&declaration.package_name),
        provides,
        requires,
    }
}

/// One entry per capability *name*. Several functions typically provide the
/// same capability (`read`/`readJson`/`readToVfs` all provide `jina.ai/read`),
/// so entries merge: fields union across providers, and the description comes
/// from the first providing function in name order.
fn provided_capabilities(declaration: &PackageDeclaration) -> Vec<ProvidedCapability> {
    let mut merged: BTreeMap<String, ProvidedCapability> = BTreeMap::new();
    for value in declaration.values.values() {
        let ValueKind::Function { params, doc, .. } = &value.kind else {
            continue;
        };
        let Some(doc) = doc else { continue };
        let param_descriptions = doc
            .params
            .iter()
            .map(|param| (param.name.as_str(), param.description.as_str()))
            .collect::<BTreeMap<_, _>>();
        for cap in &doc.capabilities {
            let entry =
                merged
                    .entry(cap.capability.clone())
                    .or_insert_with(|| ProvidedCapability {
                        name: cap.capability.clone(),
                        description: None,
                        fields: BTreeMap::new(),
                    });
            if entry.description.is_none() {
                entry.description = non_empty(doc.summary.as_str()).map(str::to_string);
            }
            for binding in &cap.bindings {
                let description = binding_description(&binding.kind, &param_descriptions);
                let field = entry
                    .fields
                    .entry(binding.field.clone())
                    .or_insert_with(|| ProvidedField {
                        ty: binding_type(&binding.kind, params),
                        description: None,
                    });
                if field.description.is_none() {
                    field.description = description;
                }
            }
        }
    }
    merged.into_values().collect()
}

fn binding_description(
    kind: &DocCapabilityBindingKind,
    param_descriptions: &BTreeMap<&str, &str>,
) -> Option<String> {
    let DocCapabilityBindingKind::Parameter { param, .. } = kind else {
        return None;
    };
    param_descriptions
        .get(param.as_str())
        .and_then(|description| non_empty(description).map(str::to_string))
}

fn binding_type(kind: &DocCapabilityBindingKind, params: &[Param]) -> String {
    match kind {
        DocCapabilityBindingKind::Parameter { param, path, .. } => params
            .iter()
            .find(|candidate| candidate.name == *param)
            .map_or_else(
                || "unknown".to_string(),
                |param| type_at_path(&param.ty, path),
            ),
        DocCapabilityBindingKind::Type { name, .. } => name.clone(),
        DocCapabilityBindingKind::Literal { value, .. } => literal_type(value).to_string(),
    }
}

// Renders the bound field's type for the provided-capability schema, stripping
// type-aliases (via `peel`) so a policy filter sees the underlying primitive
// (e.g. `string`) rather than an alias label that can't be matched against.
fn type_at_path(ty: &Type, path: &[String]) -> String {
    let mut current = ty;
    for segment in path {
        let Type::Object { fields, .. } = current.peel() else {
            return current.peel().to_string();
        };
        let Some(field) = fields.get(segment) else {
            return current.peel().to_string();
        };
        current = &field.ty;
    }
    current.peel().to_string()
}

fn literal_type(value: &DocCapabilityLiteral) -> &'static str {
    match value {
        DocCapabilityLiteral::String(_) => "string",
        DocCapabilityLiteral::Number(_) => "number",
        DocCapabilityLiteral::Boolean(_) => "boolean",
        DocCapabilityLiteral::Null => "null",
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn namespace_from_package(package_name: &str) -> String {
    package_name
        .strip_prefix('@')
        .and_then(|rest| rest.split_once('/').map(|(scope, _)| scope.to_string()))
        .unwrap_or_else(|| package_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use interpreter::{FileId, Package, Span, ValueSymbol, doc, mangle};

    #[test]
    fn schema_derives_provides_from_doc_capabilities() {
        let mut declaration = PackageDeclaration::with_package("@stripe/sdk");
        declaration.values.insert(
            "charge".to_string(),
            ValueSymbol {
                name: "charge".to_string(),
                mangled_name: mangle::package_symbol("@stripe/sdk", "charge"),
                declaration_span: Span::new(FileId(0), 0, 1),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![
                        Param::new("amount", Type::Number),
                        Param::new("currency", Type::String),
                    ],
                    ret: Type::Void,
                    type_predicate: None,
                    doc: doc(
                        FileId(0),
                        "/** Charge a customer.\n * @param amount Amount in cents.\n * @capability stripe.com/charge { amount, currency }\n */",
                    ),
                },
            },
        );

        let schema = derive_capability_schema(&declaration, &[]);

        assert_eq!(schema.namespace, "stripe");
        assert_eq!(schema.provides.len(), 1);
        assert_eq!(schema.provides[0].name, "stripe.com/charge");
        assert_eq!(schema.provides[0].fields["amount"].ty, "number");
        assert_eq!(
            schema.provides[0].fields["amount"].description.as_deref(),
            Some("Amount in cents.")
        );
    }

    #[test]
    fn same_capability_across_functions_merges_into_one_entry() {
        let mut declaration = PackageDeclaration::with_package("@jina/reader");
        for (func, comment) in [
            (
                "read",
                "/** Read a URL as markdown.\n * @param host Target host.\n * @capability jina.ai/read { host }\n */",
            ),
            (
                "readJson",
                "/** Read a URL as JSON.\n * @param host Target host.\n * @capability jina.ai/read { host, format }\n */",
            ),
        ] {
            declaration.values.insert(
                func.to_string(),
                ValueSymbol {
                    name: func.to_string(),
                    mangled_name: mangle::package_symbol("@jina/reader", func),
                    declaration_span: Span::new(FileId(0), 0, 1),
                    kind: ValueKind::Function {
                        generics: Vec::new(),
                        params: vec![
                            Param::new("host", Type::String),
                            Param::new("format", Type::String),
                        ],
                        ret: Type::Void,
                        type_predicate: None,
                        doc: doc(FileId(0), comment),
                    },
                },
            );
        }

        let schema = derive_capability_schema(&declaration, &[]);

        assert_eq!(schema.provides.len(), 1);
        let cap = &schema.provides[0];
        assert_eq!(cap.name, "jina.ai/read");
        // First providing function in name order supplies the description…
        assert_eq!(cap.description.as_deref(), Some("Read a URL as markdown."));
        // …and the fields are the union across providers.
        assert_eq!(
            cap.fields.keys().collect::<Vec<_>>(),
            ["format", "host"],
            "got {:?}",
            cap.fields
        );
    }

    #[test]
    fn provides_strips_type_aliases_to_underlying_primitive() {
        // A param typed as `type IssueId = string` must surface as `string` — a
        // policy filter can't match against an alias label.
        let id_alias = Type::alias_ty(
            Package::user(),
            "IssueId",
            mangle::package_symbol("@acme/sdk", "IssueId"),
            Vec::new(),
            Box::new(Type::String),
        );
        let mut declaration = PackageDeclaration::with_package("@acme/sdk");
        declaration.values.insert(
            "getIssue".to_string(),
            ValueSymbol {
                name: "getIssue".to_string(),
                mangled_name: mangle::package_symbol("@acme/sdk", "getIssue"),
                declaration_span: Span::new(FileId(0), 0, 1),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::new("id", id_alias)],
                    ret: Type::Void,
                    type_predicate: None,
                    doc: doc(
                        FileId(0),
                        "/** Get an issue.\n * @capability acme.com/getIssue { id }\n */",
                    ),
                },
            },
        );

        let schema = derive_capability_schema(&declaration, &[]);
        assert_eq!(schema.provides[0].fields["id"].ty, "string");
    }

    #[test]
    fn provides_preserves_explicit_array_type() {
        let mut declaration = PackageDeclaration::with_package("@acme/mail");
        declaration.values.insert(
            "send".to_string(),
            ValueSymbol {
                name: "send".to_string(),
                mangled_name: mangle::package_symbol("@acme/mail", "send"),
                declaration_span: Span::new(FileId(0), 0, 1),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::new("input", Type::Unknown)],
                    ret: Type::Void,
                    type_predicate: None,
                    doc: doc(
                        FileId(0),
                        "/** Send mail.\n * @capability acme.com/send { recipients: string[] }\n */",
                    ),
                },
            },
        );

        let schema = derive_capability_schema(&declaration, &[]);
        assert_eq!(schema.provides[0].fields["recipients"].ty, "string[]");
    }
}
