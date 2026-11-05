//! Property-name domains shared by index signatures and template literal types.
use crate::Type;
use crate::type_size::{TypeLimits, TypeTooLarge};

pub fn matches_name(domain: &Type, name: &str, limits: &TypeLimits) -> bool {
    if !limits.spend_work(name.len() as u64) {
        return false;
    }
    let Ok(units) = crate::literal_units::try_literal_units(name) else {
        limits.record(TypeTooLarge::Allocation);
        return false;
    };
    let result = matches_units(domain, &units, false, &mut |work| {
        limits
            .spend_work(work)
            .then_some(())
            .ok_or(TypeTooLarge::Work)
    });
    match result {
        Ok(value) => value,
        Err(MatchError::Work(error)) => {
            limits.record(error);
            false
        }
        Err(MatchError::Allocation) => {
            limits.record(TypeTooLarge::Allocation);
            false
        }
    }
}

pub fn canonical_number(name: &str) -> bool {
    name.parse::<f64>()
        .is_ok_and(|number| crate::runtime::number::format_number_js(number) == name)
}

#[derive(Debug)]
pub enum MatchError<E> {
    Work(E),
    Allocation,
}

/// Every scan and candidate comparison is charged before it runs. Template
/// matching uses TypeScript's first-delimiter split, including one UTF-16
/// unit for an empty delimiter between adjacent substitutions.
pub fn matches_units<E>(
    domain: &Type,
    units: &[u16],
    substitution: bool,
    charge: &mut impl FnMut(u64) -> Result<(), E>,
) -> Result<bool, MatchError<E>> {
    charge(1).map_err(MatchError::Work)?;
    Ok(match domain.peel() {
        Type::String => true,
        Type::StringLiteral(value) => {
            charge(value.len() as u64).map_err(MatchError::Work)?;
            literal_units(value)?.as_slice() == units
        }
        Type::Number | Type::NumberLiteral(_) => {
            charge(units.len() as u64).map_err(MatchError::Work)?;
            let Some(text) = decode_units(units)? else {
                return Ok(false);
            };
            match domain.peel() {
                Type::NumberLiteral(value) => {
                    crate::runtime::number::format_number_js(value.0) == text
                }
                _ if substitution => {
                    !text.is_empty()
                        && crate::runtime::number::string_to_number_js(&text).is_finite()
                }
                _ => canonical_number(&text),
            }
        }
        Type::BigInt | Type::BigIntLiteral(_) if substitution => {
            charge(units.len() as u64).map_err(MatchError::Work)?;
            let Some(text) = decode_units(units)? else {
                return Ok(false);
            };
            match domain.peel() {
                Type::BigIntLiteral(value) => value == &text,
                _ => bigint_text(&text),
            }
        }
        Type::Boolean if substitution => {
            units == [116, 114, 117, 101] || units == [102, 97, 108, 115, 101]
        }
        Type::BooleanLiteral(value) if substitution => {
            value.to_string().encode_utf16().eq(units.iter().copied())
        }
        Type::Null if substitution => units == [110, 117, 108, 108],
        Type::Undefined if substitution => units == [117, 110, 100, 101, 102, 105, 110, 101, 100],
        Type::Union(members) => {
            for member in members {
                if matches_units(member, units, substitution, charge)? {
                    return Ok(true);
                }
            }
            false
        }
        Type::TemplateLiteral {
            parts,
            substitutions,
        } => matches_template(parts, substitutions, units, charge)?,
        _ => false,
    })
}

fn matches_template<E>(
    parts: &[String],
    substitutions: &[Type],
    units: &[u16],
    charge: &mut impl FnMut(u64) -> Result<(), E>,
) -> Result<bool, MatchError<E>> {
    let Some(first) = parts.first() else {
        return Ok(false);
    };
    if substitutions.is_empty() {
        return matches_units(&Type::StringLiteral(first.clone()), units, false, charge);
    }
    let Some(last) = parts.last() else {
        return Ok(false);
    };
    charge(first.len().saturating_add(last.len()) as u64).map_err(MatchError::Work)?;
    let prefix = literal_units(first)?;
    let suffix = literal_units(last)?;
    if units.len() < prefix.len().saturating_add(suffix.len())
        || !units.starts_with(&prefix)
        || !units.ends_with(&suffix)
    {
        return Ok(false);
    }
    let remaining_end = units.len() - suffix.len();
    let mut position = prefix.len();
    for (index, hole) in substitutions.iter().enumerate() {
        let last_hole = index.saturating_add(1) == substitutions.len();
        let (end, delimiter_length) = if last_hole {
            (remaining_end, 0)
        } else {
            let Some(delimiter) = parts.get(index.saturating_add(1)) else {
                return Ok(false);
            };
            charge(delimiter.len() as u64).map_err(MatchError::Work)?;
            let delimiter = literal_units(delimiter)?;
            if delimiter.is_empty() {
                if position >= remaining_end {
                    return Ok(false);
                }
                (position + 1, 0)
            } else {
                let mut found = None;
                for candidate in position..=remaining_end {
                    charge(delimiter.len() as u64).map_err(MatchError::Work)?;
                    if units
                        .get(candidate..remaining_end)
                        .is_some_and(|tail| tail.starts_with(&delimiter))
                    {
                        found = Some(candidate);
                        break;
                    }
                }
                let Some(end) = found else {
                    return Ok(false);
                };
                (end, delimiter.len())
            }
        };
        let Some(value) = units.get(position..end) else {
            return Ok(false);
        };
        if !matches_units(hole, value, true, charge)? {
            return Ok(false);
        }
        position = end.saturating_add(delimiter_length);
    }
    Ok(true)
}

fn literal_units<E>(text: &str) -> Result<Vec<u16>, MatchError<E>> {
    crate::literal_units::try_literal_units(text).map_err(|_| MatchError::Allocation)
}

fn decode_units<E>(units: &[u16]) -> Result<Option<String>, MatchError<E>> {
    let capacity = units.len().checked_mul(3).ok_or(MatchError::Allocation)?;
    let mut text = String::new();
    text.try_reserve(capacity)
        .map_err(|_| MatchError::Allocation)?;
    for scalar in char::decode_utf16(units.iter().copied()) {
        let Ok(scalar) = scalar else {
            return Ok(None);
        };
        text.push(scalar);
    }
    Ok(Some(text))
}

fn bigint_text(text: &str) -> bool {
    let text = text.strip_prefix('-').unwrap_or(text);
    let (digits, radix) =
        if let Some(digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            (digits, 16)
        } else if let Some(digits) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
            (digits, 8)
        } else if let Some(digits) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
            (digits, 2)
        } else {
            (text, 10)
        };
    !digits.is_empty()
        && (radix != 10 || digits.len() == 1 || !digits.starts_with('0'))
        && digits
            .chars()
            .all(|digit| digit.is_ascii() && digit.is_digit(radix))
}

pub fn pattern(ty: &Type) -> Option<String> {
    match ty.peel() {
        Type::String => Some("[\\s\\S]*".into()),
        Type::Number => Some(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?".into()),
        Type::BigInt => Some(r"-?(?:[0-9]+|0[xX][0-9a-fA-F]+|0[oO][0-7]+|0[bB][01]+)".into()),
        Type::BigIntLiteral(value) => Some(regex::escape(value)),
        Type::Null => Some("null".into()),
        Type::Undefined => Some("undefined".into()),
        Type::StringLiteral(value) => Some(literal_pattern(value)),
        Type::NumberLiteral(value) => Some(regex::escape(
            &crate::runtime::number::format_number_js(value.0),
        )),
        Type::BooleanLiteral(value) => Some(value.to_string()),
        Type::Boolean => Some("(?:true|false)".into()),
        Type::Union(members) => Some(format!(
            "(?:{})",
            members
                .iter()
                .map(pattern)
                .collect::<Option<Vec<_>>>()?
                .join("|")
        )),
        Type::TemplateLiteral {
            parts,
            substitutions,
        } => {
            if parts.len() != substitutions.len().checked_add(1)? {
                return None;
            }
            let mut result = String::new();
            for (position, part) in parts.iter().enumerate() {
                result.push_str(&literal_pattern(part));
                if let Some(substitution) = substitutions.get(position) {
                    if position + 1 < substitutions.len() && parts.get(position + 1)?.is_empty() {
                        if !matches!(substitution.peel(), Type::String) {
                            return None;
                        }
                        result.push_str("[\\s\\S]");
                    } else {
                        result.push_str(&pattern(substitution)?);
                    }
                }
            }
            Some(result)
        }
        _ => None,
    }
}

fn literal_pattern(text: &str) -> String {
    let mut result = String::new();
    for unit in crate::literal_units::literal_units(text) {
        result.push_str(&format!("\\u{unit:04x}"));
    }
    result
}

/// Runtime matching needs the domain, not source alias names. Keeping aliases
/// here would consume the JSON decoder's nesting budget without adding meaning.
pub fn runtime_domain(ty: &Type) -> Result<Type, TypeTooLarge> {
    fn simplify(
        ty: &Type,
        depth: u32,
        budget: &mut crate::type_size::TypeBudget<'_>,
    ) -> Result<Type, TypeTooLarge> {
        budget.charge(depth)?;
        let simplified = crate::type_size::map_children(ty.peel(), |child| {
            simplify(child, depth.saturating_add(1), budget)
        })?;
        let simplified = match simplified {
            Type::Union(members) => Type::union(members),
            other => other,
        };
        normalize_template(simplified, budget)
    }
    let limits = TypeLimits::default();
    simplify(ty, 1, &mut limits.budget())
}

/// Collapse literal holes and distribute union holes before matching, as
/// TypeScript does when constructing a template type. Expansion consumes the
/// same budget as substitution, including the text copied into every branch.
pub fn normalize_template(
    ty: Type,
    budget: &mut crate::type_size::TypeBudget<'_>,
) -> Result<Type, TypeTooLarge> {
    let Type::TemplateLiteral {
        parts,
        substitutions,
    } = ty
    else {
        return Ok(ty);
    };
    if parts.len() != substitutions.len().saturating_add(1) {
        return Err(TypeTooLarge::InvalidTemplate);
    }
    let first = parts.first().ok_or(TypeTooLarge::InvalidTemplate)?;
    budget.charge_work(first.len() as u64)?;
    let mut builders = vec![TemplateBuilder {
        first: first.clone(),
        holes: Vec::new(),
    }];
    for (hole, suffix) in substitutions.into_iter().zip(parts.into_iter().skip(1)) {
        let alternatives = match hole.peel() {
            Type::Union(members) => members.clone(),
            Type::Boolean => vec![Type::BooleanLiteral(false), Type::BooleanLiteral(true)],
            Type::Never => return Ok(Type::Never),
            _ => vec![hole],
        };
        let mut expanded = Vec::new();
        for builder in builders {
            for alternative in &alternatives {
                budget.charge(1)?;
                budget.charge_work(
                    (builder.first.len() as u64)
                        .saturating_add(
                            builder
                                .holes
                                .iter()
                                .map(|hole| hole.suffix.len() as u64)
                                .sum::<u64>(),
                        )
                        .saturating_add(suffix.len() as u64),
                )?;
                for hole in &builder.holes {
                    budget.charge_copy(&hole.ty, 1)?;
                }
                budget.charge_copy(alternative, 1)?;
                let mut next = builder.clone();
                next.append(alternative, &suffix)?;
                expanded.push(next);
            }
        }
        builders = expanded;
    }
    Ok(Type::union(
        builders.into_iter().map(TemplateBuilder::finish).collect(),
    ))
}

#[derive(Clone)]
struct TemplateBuilder {
    first: String,
    holes: Vec<TemplateHole>,
}

#[derive(Clone)]
struct TemplateHole {
    ty: Type,
    suffix: String,
}

impl TemplateBuilder {
    fn tail(&mut self) -> &mut String {
        match self.holes.last_mut() {
            Some(hole) => &mut hole.suffix,
            None => &mut self.first,
        }
    }

    fn append(&mut self, hole: &Type, suffix: &str) -> Result<(), TypeTooLarge> {
        let literal = match hole.peel() {
            Type::StringLiteral(text) | Type::BigIntLiteral(text) => Some(text.clone()),
            Type::NumberLiteral(value) => Some(crate::runtime::number::format_number_js(value.0)),
            Type::BooleanLiteral(value) => Some(value.to_string()),
            Type::Null => Some("null".into()),
            Type::Undefined => Some("undefined".into()),
            _ => None,
        };
        if let Some(literal) = literal {
            crate::literal_units::push_literal_text(self.tail(), &literal);
            crate::literal_units::push_literal_text(self.tail(), suffix);
        } else if let Type::TemplateLiteral {
            parts,
            substitutions,
        } = hole.peel()
        {
            if parts.len() != substitutions.len().saturating_add(1) {
                return Err(TypeTooLarge::InvalidTemplate);
            }
            let first = parts.first().ok_or(TypeTooLarge::InvalidTemplate)?;
            crate::literal_units::push_literal_text(self.tail(), first);
            self.holes
                .extend(
                    substitutions
                        .iter()
                        .zip(parts.iter().skip(1))
                        .map(|(ty, suffix)| TemplateHole {
                            ty: ty.clone(),
                            suffix: suffix.clone(),
                        }),
                );
            crate::literal_units::push_literal_text(self.tail(), suffix);
        } else {
            self.holes.push(TemplateHole {
                ty: hole.clone(),
                suffix: suffix.into(),
            });
        }
        Ok(())
    }

    fn finish(self) -> Type {
        if self.holes.is_empty() {
            return Type::StringLiteral(self.first);
        }
        if self.first.is_empty()
            && self
                .holes
                .iter()
                .all(|hole| hole.suffix.is_empty() && matches!(hole.ty.peel(), Type::String))
        {
            return Type::String;
        }
        let (substitutions, suffixes): (Vec<_>, Vec<_>) = self
            .holes
            .into_iter()
            .map(|hole| (hole.ty, hole.suffix))
            .unzip();
        let parts = std::iter::once(self.first).chain(suffixes).collect();
        Type::TemplateLiteral {
            parts,
            substitutions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(parts: &[&str], substitutions: Vec<Type>) -> Type {
        Type::TemplateLiteral {
            parts: parts.iter().map(ToString::to_string).collect(),
            substitutions,
        }
    }

    #[test]
    fn numeric_names_use_canonical_javascript_spelling() {
        for name in ["0", "1.5", "1e+21", "NaN", "Infinity", "-Infinity"] {
            assert!(canonical_number(name), "{name}");
        }
        for name in ["-0", "01", "1e21", " 1", "0x10", ""] {
            assert!(!canonical_number(name), "{name}");
        }
    }

    #[test]
    fn template_matching_uses_typescript_splits_and_numeric_syntax() {
        let limits = TypeLimits::default();
        let numeric = template(&["n_", ""], vec![Type::Number]);
        for name in ["n_ ", "n_0x10", "n_-0", "n_1.5"] {
            assert!(matches_name(&numeric, name, &limits), "{name}");
        }
        for name in ["n_", "n_NaN", "n_Infinity", "n_1e999"] {
            assert!(!matches_name(&numeric, name, &limits), "{name}");
        }
        assert!(!matches_name(
            &template(&["", "", ""], vec![Type::String, Type::Number]),
            "xx42",
            &limits
        ));
        assert!(!matches_name(
            &template(&["", "_", ""], vec![Type::String, Type::Number]),
            "a_b_2",
            &limits
        ));
        let bigint = template(&["", ""], vec![Type::BigInt]);
        assert!(matches_name(&bigint, "-0x10", &limits));
        assert!(!matches_name(&bigint, "01", &limits));
        assert!(!matches_name(&bigint, "+1", &limits));
    }

    #[test]
    fn matching_preserves_lone_surrogates_and_stops_when_work_is_refused() {
        let domain = template(&["id_", ""], vec![Type::String]);
        let units = [105, 100, 95, 0xd800];
        assert!(matches_units(&domain, &units, false, &mut |_| Ok::<_, ()>(())).unwrap());
        assert!(matches!(
            matches_units(&domain, &units, false, &mut |_| Err(())),
            Err(MatchError::Work(()))
        ));
    }

    #[test]
    fn normalization_joins_literal_holes_and_checks_shape_and_work() {
        let limits = TypeLimits::default();
        let result = normalize_template(
            template(
                &["id_", "", ""],
                vec![Type::StringLiteral("abc".into()), Type::Number],
            ),
            &mut limits.budget(),
        )
        .unwrap();
        assert!(matches_name(&result, "id_abc12", &limits));
        assert!(
            normalize_template(
                template(&["missing suffix"], vec![Type::Number]),
                &mut limits.budget()
            )
            .is_err()
        );
        let exhausted = TypeLimits::with_work_allowance(0);
        assert!(
            normalize_template(
                template(&["prefix", ""], vec![Type::String]),
                &mut exhausted.budget()
            )
            .is_err()
        );
    }
}
