//! Iterative type spelling; frames borrow the input and never clone recursive types.
use crate::rendering::{RenderError, RenderLimits, RenderedText, Writer};
use crate::{IndexSignature, ObjectField, Type};

enum Frame<'a> {
    Type(&'a Type, usize),
    Text(&'a str),
    List {
        remaining: &'a [Type],
        index: usize,
        separator: &'static str,
        params: bool,
        rest: bool,
        union: bool,
        depth: usize,
    },
    Fields {
        fields: std::collections::btree_map::Iter<'a, String, ObjectField>,
        index: Option<&'a IndexSignature>,
        first: bool,
        depth: usize,
    },
}

pub(crate) fn render(ty: &Type, limits: RenderLimits) -> Result<RenderedText, RenderError> {
    Writer::render(limits, |out| write_type(out, ty))
}

pub(crate) fn write_type(out: &mut Writer, ty: &Type) -> Result<(), RenderError> {
    let mut frames = Vec::new();
    push(&mut frames, Frame::Type(ty, 1))?;
    while let Some(frame) = frames.pop() {
        out.step()?;
        match frame {
            Frame::Text(text) => out.push(text)?,
            Frame::Type(ty, depth) => {
                out.depth(depth)?;
                write_node(out, &mut frames, ty, depth)?;
            }
            Frame::List {
                remaining,
                index,
                separator,
                params,
                rest,
                union,
                depth,
            } => {
                let Some((ty, tail)) = remaining.split_first() else {
                    continue;
                };
                if index != 0 {
                    out.push(separator)?;
                }
                if params {
                    if rest && tail.is_empty() {
                        out.push("...")?;
                    }
                    out.format(format_args!("arg{index}: "))?;
                }
                push(
                    &mut frames,
                    Frame::List {
                        remaining: tail,
                        index: index.checked_add(1).ok_or(RenderError::Formatting)?,
                        separator,
                        params,
                        rest,
                        union,
                        depth,
                    },
                )?;
                let parens = union && matches!(ty, Type::Function { .. });
                if parens {
                    out.push("(")?;
                    push(&mut frames, Frame::Text(")"))?;
                }
                push(&mut frames, Frame::Type(ty, depth))?;
            }
            Frame::Fields {
                mut fields,
                index,
                first,
                depth,
            } => {
                if let Some((name, field)) = fields.next() {
                    if !first {
                        out.push("; ")?;
                    }
                    out.push(name)?;
                    out.push(if field.optional { "?: " } else { ": " })?;
                    push(
                        &mut frames,
                        Frame::Fields {
                            fields,
                            index,
                            first: false,
                            depth,
                        },
                    )?;
                    push(&mut frames, Frame::Type(&field.ty, depth))?;
                } else if let Some(index) = index {
                    if !first {
                        out.push("; ")?;
                    }
                    if index.readonly {
                        out.push("readonly ")?;
                    }
                    out.push("[key: string]: ")?;
                    push(&mut frames, Frame::Type(&index.value, depth))?;
                }
            }
        }
    }
    Ok(())
}

fn write_node<'a>(
    out: &mut Writer,
    frames: &mut Vec<Frame<'a>>,
    ty: &'a Type,
    depth: usize,
) -> Result<(), RenderError> {
    let child = depth.checked_add(1).ok_or(RenderError::Truncated)?;
    match ty {
        Type::Number => out.push("number"),
        Type::BigInt => out.push("bigint"),
        Type::BigIntLiteral(digits) => {
            out.push(digits)?;
            out.push("n")
        }
        Type::NumberLiteral(value) => out.push(&crate::runtime::number::format_number_js(value.0)),
        Type::String => out.push("string"),
        Type::StringLiteral(value) => write_string(out, value),
        Type::Uint8Array => out.push("Uint8Array"),
        Type::Boolean => out.push("boolean"),
        Type::BooleanLiteral(value) => out.push(if *value { "true" } else { "false" }),
        Type::Null => out.push("null"),
        Type::Void => out.push("void"),
        Type::Unknown => out.push("unknown"),
        Type::Error => out.push("<error>"),
        Type::Never => out.push("never"),
        Type::TypeVar(name) | Type::GenericParam { name, .. } => out.push(name),
        Type::NumberEnum { name, .. } | Type::StringEnum { name, .. } => {
            out.push(name)?;
            match ty.enum_member_name() {
                Some((_, member, _)) => {
                    out.push(".")?;
                    out.push(member)
                }
                None => Ok(()),
            }
        }
        Type::Function {
            params,
            ret,
            has_rest,
            ..
        } => {
            if *has_rest && params.is_empty() {
                return Err(RenderError::InvalidMetadata(
                    "rest signature has no parameter",
                ));
            }
            out.push("(")?;
            push(frames, Frame::Type(ret, child))?;
            push(frames, Frame::Text(") => "))?;
            push(
                frames,
                Frame::List {
                    remaining: params,
                    index: 0,
                    separator: ", ",
                    params: true,
                    rest: *has_rest,
                    union: false,
                    depth: child,
                },
            )
        }
        Type::Object { fields, index } => {
            if fields.is_empty() && index.is_none() {
                return out.push("{}");
            }
            out.push("{ ")?;
            push(frames, Frame::Text(" }"))?;
            push(
                frames,
                Frame::Fields {
                    fields: fields.iter(),
                    index: index.as_ref(),
                    first: true,
                    depth: child,
                },
            )
        }
        Type::Array(inner) => {
            let parens = matches!(
                inner.as_ref(),
                Type::Union(_) | Type::Function { .. } | Type::Readonly(_)
            );
            if parens {
                out.push("(")?;
            }
            push(frames, Frame::Text(if parens { ")[]" } else { "[]" }))?;
            push(frames, Frame::Type(inner, child))
        }
        Type::Readonly(inner) => {
            out.push("readonly ")?;
            push(frames, Frame::Type(inner, child))
        }
        Type::Tuple(types) => {
            out.push("[")?;
            push(frames, Frame::Text("]"))?;
            list(frames, types, ", ", false, child)
        }
        Type::Refined { original, ty } => {
            push(frames, Frame::Type(ty, child))?;
            push(frames, Frame::Text(" & "))?;
            push(frames, Frame::Type(original, child))
        }
        Type::InterfaceRef { name, args, .. }
        | Type::ClassRef { name, args, .. }
        | Type::Alias { name, args, .. }
        | Type::AliasRef { name, args, .. } => {
            out.push(name)?;
            if args.is_empty() {
                return Ok(());
            }
            out.push("<")?;
            push(frames, Frame::Text(">"))?;
            list(frames, args, ", ", false, child)
        }
        Type::Union(members) => list(frames, members, " | ", true, child),
    }
}

fn list<'a>(
    frames: &mut Vec<Frame<'a>>,
    types: &'a [Type],
    separator: &'static str,
    union: bool,
    depth: usize,
) -> Result<(), RenderError> {
    push(
        frames,
        Frame::List {
            remaining: types,
            index: 0,
            separator,
            params: false,
            rest: false,
            union,
            depth,
        },
    )
}
fn push<'a>(frames: &mut Vec<Frame<'a>>, frame: Frame<'a>) -> Result<(), RenderError> {
    frames.try_reserve(1).map_err(|_| RenderError::Allocation)?;
    frames.push(frame);
    Ok(())
}

pub(crate) fn write_string(out: &mut Writer, value: &str) -> Result<(), RenderError> {
    out.push("\"")?;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => out.push("\\\"")?,
            '\\' => out.push("\\\\")?,
            '\n' => out.push("\\n")?,
            '\r' => out.push("\\r")?,
            '\t' => out.push("\\t")?,
            '\u{8}' => out.push("\\b")?,
            '\u{b}' => out.push("\\v")?,
            '\u{c}' => out.push("\\f")?,
            '\0' if chars.peek().is_some_and(char::is_ascii_digit) => out.push("\\x00")?,
            '\0' => out.push("\\0")?,
            '\u{0}'..='\u{1f}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                out.format(format_args!("\\u{:04X}", u32::from(ch)))?;
            }
            ch => out.character(ch)?,
        }
    }
    out.push("\"")
}

/// Diagnostic substitutions own a separate allowance for copied nodes and text.
/// This is shared by every substitution/binding in one lifted definition.
pub(crate) struct CopyBudget {
    remaining: std::cell::Cell<usize>,
    pub(crate) types: crate::type_size::TypeLimits,
}

impl Default for CopyBudget {
    fn default() -> Self {
        Self {
            remaining: std::cell::Cell::new(RenderLimits::collection().bytes),
            types: crate::type_size::TypeLimits::default(),
        }
    }
}

impl CopyBudget {
    pub(crate) fn charge(&self, bytes: usize) -> Result<(), RenderError> {
        let remaining = self
            .remaining
            .get()
            .checked_sub(bytes)
            .ok_or(RenderError::Truncated)?;
        self.remaining.set(remaining);
        Ok(())
    }

    pub(crate) fn check(&self, root: &Type) -> Result<(), RenderError> {
        self.check_substitution(
            root,
            &crate::typechecker::type_param_substitution::TypeParamSubstitution::new(),
        )
    }

    /// Mirrors substitution's var-to-var cycle rule without constructing types.
    /// Validate the expanded result before the recursive copier can allocate it.
    pub(crate) fn check_substitution<'a>(
        &self,
        root: &'a Type,
        substitution: &'a crate::typechecker::type_param_substitution::TypeParamSubstitution,
    ) -> Result<(), RenderError> {
        use crate::compiler_limits::{MAX_TYPE_DEPTH, MAX_TYPE_NODES};
        let mut pending = Vec::new();
        let mut open = Vec::<&str>::new();
        pending
            .try_reserve(1)
            .map_err(|_| RenderError::Allocation)?;
        pending.push(CopyFrame::Type(root, 1u32));
        let mut remaining = MAX_TYPE_NODES;
        while let Some(frame) = pending.pop() {
            let CopyFrame::Type(ty, depth) = frame else {
                open.pop();
                continue;
            };
            remaining = remaining.checked_sub(1).ok_or(RenderError::Truncated)?;
            if depth > MAX_TYPE_DEPTH {
                return Err(RenderError::Truncated);
            }
            self.charge(std::mem::size_of::<Type>())?;
            if let Type::TypeVar(name) = ty
                && let Some(bound) = substitution.get(name)
                && !open.contains(&name.as_str())
            {
                if open.len() >= MAX_TYPE_DEPTH as usize {
                    return Err(RenderError::Truncated);
                }
                self.charge(name.len())?;
                pending
                    .try_reserve(2)
                    .map_err(|_| RenderError::Allocation)?;
                open.try_reserve(1).map_err(|_| RenderError::Allocation)?;
                open.push(name);
                pending.push(CopyFrame::LeaveBinding);
                pending.push(CopyFrame::Type(bound, depth));
                continue;
            }
            validate_function_metadata(ty)?;
            let children = child_count(ty);
            let frontier = children
                .checked_add(pending.len())
                .ok_or(RenderError::Truncated)?;
            if u64::try_from(frontier).map_err(|_| RenderError::Truncated)? > remaining {
                return Err(RenderError::Truncated);
            }
            self.charge(copied_text_bytes(ty)?)?;
            pending
                .try_reserve(children)
                .map_err(|_| RenderError::Allocation)?;
            crate::type_size::for_each_child(ty, |child| {
                pending.push(CopyFrame::Type(child, depth + 1));
            });
        }
        Ok(())
    }
}

enum CopyFrame<'a> {
    Type(&'a Type, u32),
    LeaveBinding,
}

pub(crate) fn check_for_copy(root: &Type) -> Result<(), RenderError> {
    CopyBudget::default().check(root)
}

fn validate_function_metadata(ty: &Type) -> Result<(), RenderError> {
    if let Type::Function {
        params,
        predicate,
        has_rest,
        ..
    } = ty
    {
        if *has_rest && params.is_empty() {
            return Err(RenderError::InvalidMetadata("rest parameter is absent"));
        }
        if predicate.as_ref().is_some_and(|predicate| {
            usize::try_from(predicate.parameter_index)
                .ok()
                .and_then(|index| params.get(index))
                .is_none()
        }) {
            return Err(RenderError::InvalidMetadata(
                "predicate parameter is absent",
            ));
        }
    }
    Ok(())
}

fn copied_text_bytes(ty: &Type) -> Result<usize, RenderError> {
    let mut bytes = 0usize;
    let mut add = |text: &str| {
        bytes = bytes
            .checked_add(text.len())
            .ok_or(RenderError::Truncated)?;
        Ok::<(), RenderError>(())
    };
    match ty {
        Type::StringLiteral(text) | Type::TypeVar(text) | Type::GenericParam { name: text, .. } => {
            add(text)?;
        }
        Type::Object { fields, .. } => {
            for name in fields.keys() {
                add(name)?;
            }
        }
        Type::InterfaceRef {
            mangled,
            package,
            name,
            ..
        }
        | Type::ClassRef {
            mangled,
            package,
            name,
            ..
        }
        | Type::AliasRef {
            mangled,
            package,
            name,
            ..
        }
        | Type::Alias {
            mangled,
            package,
            name,
            ..
        }
        | Type::NumberEnum {
            mangled,
            package,
            name,
            ..
        }
        | Type::StringEnum {
            mangled,
            package,
            name,
            ..
        } => {
            add(mangled.as_str())?;
            add(package.as_str())?;
            add(name)?;
        }
        _ => {}
    }
    Ok(bytes)
}

fn child_count(ty: &Type) -> usize {
    match ty {
        Type::Function {
            params, predicate, ..
        } => params
            .len()
            .saturating_add(1 + usize::from(predicate.is_some())),
        Type::Object { fields, index } => fields.len().saturating_add(usize::from(index.is_some())),
        Type::Array(_) | Type::Readonly(_) => 1,
        Type::Tuple(members) | Type::Union(members) => members.len(),
        Type::Refined { .. } => 2,
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => args.len(),
        Type::Alias { args, .. } => args.len().saturating_add(1),
        Type::Number
        | Type::NumberLiteral(_)
        | Type::BigInt
        | Type::BigIntLiteral(_)
        | Type::String
        | Type::StringLiteral(_)
        | Type::Uint8Array
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Null
        | Type::Void
        | Type::Unknown
        | Type::Error
        | Type::Never
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. } => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_budget_bounds_text_aggregate_inputs_and_expansion() {
        use crate::typechecker::type_param_substitution::TypeParamSubstitution;
        let huge = Type::StringLiteral("x".repeat(2 * 1024 * 1024));
        assert!(matches!(check_for_copy(&huge), Err(RenderError::Truncated)));
        let chunk = Type::StringLiteral("x".repeat(600 * 1024));
        let budget = CopyBudget::default();
        assert!(budget.check(&chunk).is_ok());
        assert!(matches!(budget.check(&chunk), Err(RenderError::Truncated)));
        let sub = TypeParamSubstitution::from_pairs(&["T".into()], &[chunk]);
        let repeated = Type::Tuple(vec![Type::TypeVar("T".into()); 2]);
        assert!(matches!(
            CopyBudget::default().check_substitution(&repeated, &sub),
            Err(RenderError::Truncated)
        ));
        let cycle = TypeParamSubstitution::from_pairs(&["T".into()], &[Type::TypeVar("T".into())]);
        assert!(
            CopyBudget::default()
                .check_substitution(&Type::TypeVar("T".into()), &cycle)
                .is_ok()
        );
    }

    #[test]
    fn copy_preflight_rejects_wide_frontiers_and_hidden_depth() {
        let wide = Type::Tuple(vec![Type::Number; 100_000]);
        assert!(matches!(check_for_copy(&wide), Err(RenderError::Truncated)));
        assert!(check_for_copy(&Type::Number).is_ok());
        let mut deep = Type::Number;
        for _ in 0..600 {
            deep = Type::Array(Box::new(deep));
        }
        assert!(matches!(check_for_copy(&deep), Err(RenderError::Truncated)));
        while let Type::Array(inner) = deep {
            deep = *inner;
        }
    }
}
