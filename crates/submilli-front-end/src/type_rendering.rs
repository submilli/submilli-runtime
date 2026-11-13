//! Iterative type spelling; frames borrow the input and never clone recursive types.
use crate::local_type_names::{display_type_name, written_type_args};
use crate::rendering::{RenderError, RenderLimits, RenderedText, Writer};
use crate::types::MappedModifier;
use crate::{IndexSignature, ObjectField, Type};

/// What a [`Frame::List`] writes, which decides how each item is spelled.
#[derive(Clone, Copy)]
enum ListKind {
    /// Type arguments.
    Plain,
    /// Union members: a function, refinement or conditional member is
    /// parenthesized.
    Union,
    /// Intersection members: parenthesized as a union's, and a union member
    /// too.
    Intersection,
    /// A function's parameters, named `argN`; items from `optional_from` on
    /// are optional, and the last is the rest parameter when `rest`.
    Params { rest: bool, optional_from: usize },
    /// Tuple positions; items from `optional_from` on are optional.
    Tuple { optional_from: usize },
}

/// An object type's index signatures, `index` then `more_indexes`.
type IndexSignatures<'a> =
    std::iter::Chain<std::option::Iter<'a, IndexSignature>, std::slice::Iter<'a, IndexSignature>>;

enum Frame<'a> {
    Type(&'a Type, usize),
    Text(&'a str),
    TemplateText(&'a str),
    List {
        /// The items left to write, last first, so `pop` yields the next.
        remaining: Vec<&'a Type>,
        index: usize,
        separator: &'static str,
        kind: ListKind,
        depth: usize,
    },
    /// A type written beside a `?` marker, which already admits `undefined`:
    /// a union drops its `undefined` member. The rule of
    /// [`crate::types::shown_beside_optional_marker`], applied to borrowed
    /// members because frames can't hold the owned type it returns.
    Optional(&'a Type, usize),
    Fields {
        fields: std::collections::btree_map::Iter<'a, String, ObjectField>,
        indexes: IndexSignatures<'a>,
        first: bool,
        depth: usize,
    },
    /// Starts a conditional type's `extends` clause, where its `infer`
    /// names are spelled `infer N`.
    EnterInfers(&'a [String]),
    LeaveInfers,
}

pub fn render(ty: &Type, limits: RenderLimits) -> Result<RenderedText, RenderError> {
    Writer::render(limits, |out| write_type(out, ty))
}

pub fn write_type(out: &mut Writer, ty: &Type) -> Result<(), RenderError> {
    let mut frames = Vec::new();
    let mut infers: Vec<&[String]> = Vec::new();
    push(&mut frames, Frame::Type(ty, 1))?;
    while let Some(frame) = frames.pop() {
        out.step()?;
        match frame {
            Frame::Text(text) => out.push(text)?,
            Frame::TemplateText(text) => out.push(
                &crate::types::escape_string_literal(text)
                    .replace('`', "\\`")
                    .replace("${", "\\${"),
            )?,
            Frame::Type(ty, depth) => {
                out.depth(depth)?;
                write_node(out, &mut frames, ty, depth, &infers)?;
            }
            Frame::EnterInfers(names) => {
                infers.try_reserve(1).map_err(|_| RenderError::Allocation)?;
                infers.push(names);
            }
            Frame::LeaveInfers => {
                infers.pop();
            }
            Frame::List {
                mut remaining,
                index,
                separator,
                kind,
                depth,
            } => {
                let Some(ty) = remaining.pop() else {
                    continue;
                };
                let last = remaining.is_empty();
                if index != 0 {
                    out.push(separator)?;
                }
                let next = index.checked_add(1).ok_or(RenderError::Formatting)?;
                push(
                    &mut frames,
                    Frame::List {
                        remaining,
                        index: next,
                        separator,
                        kind,
                        depth,
                    },
                )?;
                match kind {
                    ListKind::Plain => push(&mut frames, Frame::Type(ty, depth))?,
                    ListKind::Union | ListKind::Intersection => {
                        let parens = matches!(
                            ty,
                            Type::Function { .. } | Type::Refined { .. } | Type::Conditional { .. }
                        ) || (matches!(kind, ListKind::Intersection)
                            && matches!(ty, Type::Union(_)));
                        if parens {
                            out.push("(")?;
                            push(&mut frames, Frame::Text(")"))?;
                        }
                        push(&mut frames, Frame::Type(ty, depth))?;
                    }
                    ListKind::Params {
                        rest,
                        optional_from,
                    } => {
                        let is_rest = rest && last;
                        if is_rest {
                            out.push("...")?;
                        }
                        if index >= optional_from && !is_rest {
                            out.format(format_args!("arg{index}?: "))?;
                            push(&mut frames, Frame::Optional(ty, depth))?;
                        } else {
                            out.format(format_args!("arg{index}: "))?;
                            push(&mut frames, Frame::Type(ty, depth))?;
                        }
                    }
                    ListKind::Tuple { optional_from } => {
                        if index >= optional_from {
                            out.push("(")?;
                            push(&mut frames, Frame::Text(")?"))?;
                        }
                        push(&mut frames, Frame::Type(ty, depth))?;
                    }
                }
            }
            Frame::Optional(ty, depth) => {
                out.depth(depth)?;
                match ty.peel() {
                    Type::Union(members)
                        if members.iter().any(|m| matches!(m.peel(), Type::Undefined)) =>
                    {
                        let present: Vec<&Type> = crate::types::union_display_order(members)
                            .into_iter()
                            .filter(|m| !matches!(m.peel(), Type::Undefined))
                            .collect();
                        match present.as_slice() {
                            [only] => push(&mut frames, Frame::Type(only, depth))?,
                            _ => push(&mut frames, union_list(present, depth))?,
                        }
                    }
                    _ => write_node(out, &mut frames, ty, depth, &infers)?,
                }
            }
            Frame::Fields {
                mut fields,
                mut indexes,
                first,
                depth,
            } => {
                if let Some((name, field)) = fields.next() {
                    if !first {
                        out.push("; ")?;
                    }
                    if field.readonly {
                        out.push("readonly ")?;
                    }
                    write_field_name(out, name)?;
                    out.push(if field.optional { "?: " } else { ": " })?;
                    push(
                        &mut frames,
                        Frame::Fields {
                            fields,
                            indexes,
                            first: false,
                            depth,
                        },
                    )?;
                    push(&mut frames, Frame::Type(&field.ty, depth))?;
                } else if let Some(index) = indexes.next() {
                    push(
                        &mut frames,
                        Frame::Fields {
                            fields,
                            indexes,
                            first: false,
                            depth,
                        },
                    )?;
                    if !first {
                        out.push("; ")?;
                    }
                    if index.readonly {
                        out.push("readonly ")?;
                    }
                    out.push("[key: ")?;
                    push(&mut frames, Frame::Type(&index.value, depth))?;
                    push(&mut frames, Frame::Text("]: "))?;
                    push(&mut frames, Frame::Type(&index.key, depth))?;
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
    infers: &[&[String]],
) -> Result<(), RenderError> {
    let child = depth.checked_add(1).ok_or(RenderError::Truncated)?;
    match ty {
        Type::Number => out.push("number"),
        Type::BigInt => out.push("bigint"),
        Type::BigIntLiteral(digits) => {
            out.push(digits)?;
            out.push("n")
        }
        Type::NumberLiteral(value) => out.push(&crate::number_format::format_number_js(value.0)),
        Type::String => out.push("string"),
        Type::TemplateLiteral {
            parts,
            substitutions,
        } => {
            if parts.len() != substitutions.len().saturating_add(1) {
                return Err(RenderError::InvalidMetadata("template part count"));
            }
            push(frames, Frame::Text("`"))?;
            for (position, part) in parts.iter().enumerate().rev() {
                push(frames, Frame::TemplateText(part))?;
                if let Some(inner) = position.checked_sub(1).and_then(|i| substitutions.get(i)) {
                    push(frames, Frame::Text("}"))?;
                    push(frames, Frame::Type(inner, child))?;
                    push(frames, Frame::Text("${"))?;
                }
            }
            out.push("`")
        }
        Type::StringLiteral(value) => write_string(out, value),
        Type::Uint8Array => out.push("Uint8Array"),
        Type::Boolean => out.push("boolean"),
        Type::BooleanLiteral(value) => out.push(if *value { "true" } else { "false" }),
        Type::Null => out.push("null"),
        Type::Undefined => out.push("undefined"),
        Type::Void => out.push("void"),
        Type::Unknown => out.push("unknown"),
        Type::NonPrimitive => out.push("object"),
        Type::Error => out.push("<error>"),
        Type::Never => out.push("never"),
        Type::TypeVar(name) => {
            if is_infer(ty, infers) {
                out.push("infer ")?;
            }
            out.push(display_type_name(name))
        }
        Type::GenericParam { name, .. } => out.push(display_type_name(name)),
        Type::KeyOf(inner) => {
            let parens = matches!(
                inner.as_ref(),
                Type::Union(_)
                    | Type::Function { .. }
                    | Type::Refined { .. }
                    | Type::Conditional { .. }
                    | Type::Intersection(_)
            ) || is_infer(inner, infers);
            out.push(if parens { "keyof (" } else { "keyof " })?;
            if parens {
                push(frames, Frame::Text(")"))?;
            }
            push(frames, Frame::Type(inner, child))
        }
        Type::IndexedAccess { object, index } => {
            let parens = matches!(
                object.as_ref(),
                Type::Union(_)
                    | Type::Function { .. }
                    | Type::Refined { .. }
                    | Type::KeyOf(_)
                    | Type::Conditional { .. }
                    | Type::Intersection(_)
            ) || is_infer(object, infers);
            if parens {
                out.push("(")?;
            }
            push(frames, Frame::Text("]"))?;
            push(frames, Frame::Type(index, child))?;
            push(frames, Frame::Text(if parens { ")[" } else { "[" }))?;
            push(frames, Frame::Type(object, child))
        }
        // `{ readonly [K in keyof T as N]?: T[K] }`, as tsc prints it.
        Type::Mapped {
            key,
            constraint,
            name,
            template,
            optional,
            readonly,
        } => {
            out.push(match readonly {
                MappedModifier::Keep => "{ [",
                MappedModifier::Add => "{ readonly [",
                MappedModifier::Remove => "{ -readonly [",
            })?;
            out.push(display_type_name(key))?;
            out.push(" in ")?;
            push(frames, Frame::Text(" }"))?;
            push(frames, Frame::Type(template, child))?;
            push(
                frames,
                Frame::Text(match optional {
                    MappedModifier::Keep => "]: ",
                    MappedModifier::Add => "]?: ",
                    MappedModifier::Remove => "]-?: ",
                }),
            )?;
            if let Some(name) = name {
                push(frames, Frame::Type(name, child))?;
                push(frames, Frame::Text(" as "))?;
            }
            push(frames, Frame::Type(constraint, child))
        }
        // The check and `extends` types bind tighter than the conditional,
        // so a function or conditional there is parenthesized.
        Type::Conditional {
            check,
            extends,
            infers: names,
            true_ty,
            false_ty,
        } => {
            let operand =
                |ty: &Type| matches!(ty, Type::Function { .. } | Type::Conditional { .. });
            let (check_parens, extends_parens) = (operand(check), operand(extends));
            push(frames, Frame::Type(false_ty, child))?;
            push(frames, Frame::Text(" : "))?;
            push(frames, Frame::Type(true_ty, child))?;
            push(frames, Frame::Text(" ? "))?;
            if extends_parens {
                push(frames, Frame::Text(")"))?;
            }
            push(frames, Frame::LeaveInfers)?;
            push(frames, Frame::Type(extends, child))?;
            push(frames, Frame::EnterInfers(names))?;
            if extends_parens {
                push(frames, Frame::Text("("))?;
            }
            push(frames, Frame::Text(" extends "))?;
            if check_parens {
                push(frames, Frame::Text(")"))?;
            }
            push(frames, Frame::Type(check, child))?;
            if check_parens {
                out.push("(")?;
            }
            Ok(())
        }
        Type::Intersection(members) => push(
            frames,
            Frame::List {
                remaining: members.iter().rev().collect(),
                index: 0,
                separator: " & ",
                kind: ListKind::Intersection,
                depth: child,
            },
        ),
        Type::NumberEnum { name, .. } | Type::StringEnum { name, .. } => {
            out.push(display_type_name(name))?;
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
            optional,
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
            let fixed = params.len().saturating_sub(usize::from(*has_rest));
            push(
                frames,
                Frame::List {
                    remaining: params.iter().rev().collect(),
                    index: 0,
                    separator: ", ",
                    kind: ListKind::Params {
                        rest: *has_rest,
                        optional_from: fixed.saturating_sub(*optional),
                    },
                    depth: child,
                },
            )
        }
        Type::Object {
            fields,
            index,
            more_indexes,
        } => {
            if fields.is_empty() && index.is_none() {
                return out.push("{}");
            }
            out.push("{ ")?;
            push(frames, Frame::Text(" }"))?;
            push(
                frames,
                Frame::Fields {
                    fields: fields.iter(),
                    indexes: index.iter().chain(more_indexes),
                    first: true,
                    depth: child,
                },
            )
        }
        Type::Array(inner) => {
            let parens = match inner.as_ref() {
                // `ReadonlyMap<K, V>[]` needs no parentheses.
                Type::Readonly(element) => !matches!(element.as_ref(), Type::InterfaceRef { .. }),
                Type::Union(_)
                | Type::Function { .. }
                | Type::Refined { .. }
                | Type::KeyOf(_)
                | Type::Conditional { .. }
                | Type::Intersection(_) => true,
                inner => is_infer(inner, infers),
            };
            if parens {
                out.push("(")?;
            }
            push(frames, Frame::Text(if parens { ")[]" } else { "[]" }))?;
            push(frames, Frame::Type(inner, child))
        }
        Type::Readonly(inner) => {
            // A readonly `Map` or `Set` is spelled `ReadonlyMap` / `ReadonlySet`.
            out.push(if matches!(inner.as_ref(), Type::InterfaceRef { .. }) {
                "Readonly"
            } else {
                "readonly "
            })?;
            push(frames, Frame::Type(inner, child))
        }
        Type::Tuple(tuple) => {
            out.push("[")?;
            push(frames, Frame::Text("]"))?;
            push(
                frames,
                Frame::List {
                    remaining: tuple.elements.iter().rev().collect(),
                    index: 0,
                    separator: ", ",
                    kind: ListKind::Tuple {
                        optional_from: tuple.required_len(),
                    },
                    depth: child,
                },
            )
        }
        // `T & ({} | null)`, as tsc prints an intersection with a union.
        Type::Refined { original, ty } => {
            let parens = matches!(ty.as_ref(), Type::Union(_));
            if parens {
                push(frames, Frame::Text(")"))?;
            }
            push(frames, Frame::Type(ty, child))?;
            push(frames, Frame::Text(if parens { " & (" } else { " & " }))?;
            push(frames, Frame::Type(original, child))
        }
        Type::InterfaceRef { name, args, .. }
        | Type::ClassRef { name, args, .. }
        | Type::Alias { name, args, .. }
        | Type::AliasRef { name, args, .. } => {
            out.push(display_type_name(name))?;
            let args = written_type_args(name, args);
            if args.is_empty() {
                return Ok(());
            }
            out.push("<")?;
            push(frames, Frame::Text(">"))?;
            push(
                frames,
                Frame::List {
                    remaining: args.iter().rev().collect(),
                    index: 0,
                    separator: ", ",
                    kind: ListKind::Plain,
                    depth: child,
                },
            )
        }
        // `null` and `undefined` print last, as tsc prints them.
        Type::Union(members) => push(
            frames,
            union_list(crate::types::union_display_order(members), child),
        ),
    }
}

/// Whether `ty` is a name an enclosing conditional's `extends` clause
/// declares with `infer`.
fn is_infer(ty: &Type, infers: &[&[String]]) -> bool {
    matches!(ty, Type::TypeVar(name) if infers.iter().any(|names| names.contains(name)))
}

fn union_list(members: Vec<&Type>, depth: usize) -> Frame<'_> {
    Frame::List {
        remaining: members.into_iter().rev().collect(),
        index: 0,
        separator: " | ",
        kind: ListKind::Union,
        depth,
    }
}

fn push<'a>(frames: &mut Vec<Frame<'a>>, frame: Frame<'a>) -> Result<(), RenderError> {
    frames.try_reserve(1).map_err(|_| RenderError::Allocation)?;
    frames.push(frame);
    Ok(())
}

/// A field name as TypeScript prints it: bare when it is an identifier (keywords
/// included), otherwise a quoted string, as `"a b"` and `"1"` are.
fn write_field_name(out: &mut Writer, name: &str) -> Result<(), RenderError> {
    if is_identifier_name(name) {
        out.push(name)
    } else {
        write_string(out, name)
    }
}

/// A member name as TypeScript prints it: bare when it is an identifier,
/// quoted otherwise.
pub fn render_field_name(name: &str) -> Result<String, RenderError> {
    Writer::render(RenderLimits::default(), |out| write_field_name(out, name)).map(|r| r.text)
}

/// Whether a member name can be written bare, as an identifier (keywords
/// included), rather than as a quoted string.
pub fn is_identifier_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c == '$' || unicode_ident::is_xid_start(c))
        && chars.all(|c| c == '_' || c == '$' || unicode_ident::is_xid_continue(c))
}

pub fn write_string(out: &mut Writer, value: &str) -> Result<(), RenderError> {
    out.push("\"")?;
    let mut chars = crate::literal_units::literal_chars(value).peekable();
    while let Some(ch) = chars.next() {
        let ch = match ch {
            Ok(ch) => ch,
            Err(lone) => {
                out.format(format_args!("\\u{lone:04X}"))?;
                continue;
            }
        };
        match ch {
            '"' => out.push("\\\"")?,
            '\\' => out.push("\\\\")?,
            '\n' => out.push("\\n")?,
            '\r' => out.push("\\r")?,
            '\t' => out.push("\\t")?,
            '\u{8}' => out.push("\\b")?,
            '\u{b}' => out.push("\\v")?,
            '\u{c}' => out.push("\\f")?,
            '\0' if chars
                .peek()
                .is_some_and(|next| next.is_ok_and(|d| d.is_ascii_digit())) =>
            {
                out.push("\\x00")?;
            }
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
pub struct CopyBudget {
    remaining: std::cell::Cell<usize>,
    pub types: crate::type_size::TypeLimits,
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
    pub fn charge(&self, bytes: usize) -> Result<(), RenderError> {
        let remaining = self
            .remaining
            .get()
            .checked_sub(bytes)
            .ok_or(RenderError::Truncated)?;
        self.remaining.set(remaining);
        Ok(())
    }

    pub fn check(&self, root: &Type) -> Result<(), RenderError> {
        self.check_substitution(root, |_| None)
    }

    /// Mirrors substitution's var-to-var cycle rule without constructing types.
    /// Validate the expanded result before the recursive copier can allocate it.
    pub fn check_substitution<'a>(
        &self,
        root: &'a Type,
        substitution: impl Fn(&str) -> Option<&'a Type>,
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
                && let Some(bound) = substitution(name)
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
            let children = crate::type_size::child_count(ty);
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
                pending.push(CopyFrame::Type(child, depth.saturating_add(1)));
            });
        }
        Ok(())
    }
}

enum CopyFrame<'a> {
    Type(&'a Type, u32),
    LeaveBinding,
}

pub fn check_for_copy(root: &Type) -> Result<(), RenderError> {
    CopyBudget::default().check(root)
}

fn validate_function_metadata(ty: &Type) -> Result<(), RenderError> {
    let Type::Function {
        params,
        ret,
        predicate,
        has_rest,
        ..
    } = ty
    else {
        return Ok(());
    };
    if *has_rest && params.is_empty() {
        return Err(RenderError::InvalidMetadata("rest parameter is absent"));
    }
    match predicate {
        Some(predicate) => validate_predicate(predicate, params, ret),
        None => Ok(()),
    }
}

fn validate_predicate(
    predicate: &crate::TypePredicate,
    params: &[Type],
    ret: &Type,
) -> Result<(), RenderError> {
    let parameter_present = usize::try_from(predicate.parameter_index)
        .ok()
        .is_some_and(|index| index < params.len());
    if !parameter_present {
        return Err(RenderError::InvalidMetadata(
            "predicate parameter is absent",
        ));
    }
    if predicate.kind.is_assertion() && !ret.is_void() {
        return Err(RenderError::InvalidMetadata(
            "assertion does not return void",
        ));
    }
    if predicate.kind == crate::PredicateKind::AssertsTruthy
        && !matches!(predicate.asserted_type, Type::Unknown)
    {
        return Err(RenderError::InvalidMetadata(
            "truthy assertion asserts a type",
        ));
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
        Type::Mapped { key, .. } => add(key)?,
        Type::Conditional { infers, .. } => {
            for name in infers {
                add(name)?;
            }
        }
        _ => {}
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_budget_bounds_text_aggregate_inputs_and_expansion() {
        let huge = Type::StringLiteral("x".repeat(2 * 1024 * 1024));
        assert!(matches!(check_for_copy(&huge), Err(RenderError::Truncated)));
        let chunk = Type::StringLiteral("x".repeat(600 * 1024));
        let budget = CopyBudget::default();
        assert!(budget.check(&chunk).is_ok());
        assert!(matches!(budget.check(&chunk), Err(RenderError::Truncated)));
        let sub = std::collections::BTreeMap::from([("T", chunk)]);
        let repeated = Type::Tuple(crate::types::TupleType {
            elements: vec![Type::TypeVar("T".into()); 2],
            optional: 0,
        });
        assert!(matches!(
            CopyBudget::default().check_substitution(&repeated, |name| sub.get(name)),
            Err(RenderError::Truncated)
        ));
        let cycle = std::collections::BTreeMap::from([("T", Type::TypeVar("T".into()))]);
        assert!(
            CopyBudget::default()
                .check_substitution(&Type::TypeVar("T".into()), |name| cycle.get(name))
                .is_ok()
        );
    }

    #[test]
    fn type_operators_render_as_tsc_spells_them() {
        use crate::types::MappedModifier;
        let var = |name: &str| Type::TypeVar(name.into());
        let keys = Type::KeyOf(Box::new(var("T")));
        let mapped = |name: Option<Type>, optional, readonly| Type::Mapped {
            key: "K".into(),
            constraint: Box::new(keys.clone()),
            name: name.map(Box::new),
            template: Box::new(Type::indexed_access(var("T"), var("K"))),
            optional,
            readonly,
        };
        let brand = Type::Object {
            more_indexes: Vec::new(),
            fields: [(
                "brand".to_string(),
                ObjectField::required(Type::BooleanLiteral(true)),
            )]
            .into(),
            index: None,
        };
        let conditional =
            |check: Type, extends: Type, infers: &[&str], true_ty: Type| Type::Conditional {
                check: Box::new(check),
                extends: Box::new(extends),
                infers: infers.iter().map(ToString::to_string).collect(),
                true_ty: Box::new(true_ty),
                false_ty: Box::new(Type::Boolean),
            };
        let string_test = conditional(var("T"), Type::String, &[], Type::Number);
        let function = Type::Function {
            params: Vec::new(),
            optional: 0,
            ret: Box::new(Type::Void),
            predicate: None,
            has_rest: false,
        };
        let cases = [
            (
                mapped(None, MappedModifier::Add, MappedModifier::Keep),
                "{ [K in keyof T]?: T[K] }",
            ),
            (
                mapped(Some(var("K")), MappedModifier::Remove, MappedModifier::Add),
                "{ readonly [K in keyof T as K]-?: T[K] }",
            ),
            (
                mapped(None, MappedModifier::Keep, MappedModifier::Remove),
                "{ -readonly [K in keyof T]: T[K] }",
            ),
            (string_test.clone(), "T extends string ? number : boolean"),
            (
                conditional(var("T"), Type::Array(Box::new(var("U"))), &["U"], var("U")),
                "T extends (infer U)[] ? U : boolean",
            ),
            (
                conditional(function.clone(), function, &[], Type::Number),
                "(() => void) extends (() => void) ? number : boolean",
            ),
            (Type::intersection(vec![var("U"), var("T")]), "U & T"),
            (
                Type::intersection(vec![brand.clone(), Type::String]),
                "{ brand: true } & string",
            ),
            (
                Type::intersection(vec![
                    var("T"),
                    Type::union(vec![Type::Number, Type::String]),
                ]),
                "T & (number | string)",
            ),
            (
                Type::Array(Box::new(Type::intersection(vec![var("T"), var("U")]))),
                "(T & U)[]",
            ),
            (
                Type::union(vec![string_test.clone(), Type::Null]),
                "(T extends string ? number : boolean) | null",
            ),
            (
                Type::KeyOf(Box::new(string_test)),
                "keyof (T extends string ? number : boolean)",
            ),
        ];
        for (ty, spelled) in cases {
            assert_eq!(ty.to_string(), spelled);
        }
    }

    #[test]
    fn intersection_flattens_and_deduplicates_without_reordering() {
        let var = |name: &str| Type::TypeVar(name.into());
        let nested = Type::intersection(vec![var("U"), var("T")]);
        assert_ne!(nested, Type::intersection(vec![var("T"), var("U")]));
        assert_eq!(
            Type::intersection(vec![var("V"), nested.clone(), Type::Unknown, var("T")]),
            Type::Intersection(vec![var("V"), var("U"), var("T")])
        );
        assert_eq!(Type::intersection(vec![var("T"), Type::Unknown]), var("T"));
        assert_eq!(Type::intersection(vec![var("T"), Type::Never]), Type::Never);
        assert_eq!(Type::intersection(Vec::new()), Type::Unknown);
        let a = || Type::StringLiteral("a".into());
        for uninhabited in [
            vec![Type::Number, a()],
            vec![Type::String, Type::Null],
            vec![Type::Undefined, Type::Null],
            vec![a(), Type::StringLiteral("b".into())],
        ] {
            assert_eq!(Type::intersection(uninhabited), Type::Never);
        }
        assert_eq!(Type::intersection(vec![Type::String, a()]), a());
        let void_undefined = Type::intersection(vec![Type::Void, Type::Undefined]);
        assert!(matches!(void_undefined, Type::Intersection(_)));
        assert_eq!(
            Type::intersection(vec![Type::Void, Type::Null]),
            Type::Never
        );
        let brand = Type::Object {
            more_indexes: Vec::new(),
            fields: [("b".to_string(), ObjectField::required(Type::Boolean))].into(),
            index: None,
        };
        // Dropping the base keeps the brand, and a refinement that peels to
        // the base is more than the base.
        assert_eq!(
            Type::intersection(vec![Type::String, a(), brand.clone()]),
            Type::Intersection(vec![a(), brand.clone()])
        );
        let refined = Type::Refined {
            original: Box::new(var("T")),
            ty: Box::new(Type::String),
        };
        assert_eq!(
            Type::intersection(vec![refined.clone(), a()]),
            Type::Intersection(vec![refined, a()])
        );
        // Built by hand, unreduced: the literal is the narrower primitive.
        let unreduced = Type::Intersection(vec![Type::String, a(), brand.clone()]);
        assert_eq!(unreduced.peel(), &a());
        // Each level is peeled once: a non-flat chain doesn't take
        // exponential time.
        let mut chain = Type::String;
        for _ in 0..200 {
            chain = Type::Intersection(vec![brand.clone(), chain]);
        }
        assert_eq!(chain.peel(), &Type::String);
        let branded = Type::intersection(vec![var("T"), Type::String]);
        assert_eq!(branded.peel(), &Type::String);
        assert_eq!(branded.lowered(), &Type::String);
        assert!(!branded.lowers_to_erased_slot());
        assert!(nested.lowers_to_erased_slot());
        assert!(nested.is_type_operator() && branded.is_type_operator());
    }

    #[test]
    fn copy_preflight_rejects_wide_frontiers_and_hidden_depth() {
        let wide = Type::Tuple(crate::types::TupleType {
            elements: vec![Type::Number; 100_000],
            optional: 0,
        });
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
