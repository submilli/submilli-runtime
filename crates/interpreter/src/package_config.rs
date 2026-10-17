//! Package-local build parameters. Token substitution retains the original source
//! spans, so diagnostics and authority maps refer to the author's config file.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::compiler_error::CompileError;
use crate::{Asi, Diagnostic, FileId, Severity, Span, Token, TokenKind};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
    /// Compiler literal text, including lossless lone-surrogate escapes.
    String(#[serde(with = "string_value")] String),
    Number(f64),
    Boolean(bool),
}

impl PartialEq for ConfigValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Number(left), Self::Number(right)) => left.to_bits() == right.to_bits(),
            (Self::Boolean(left), Self::Boolean(right)) => left == right,
            _ => false,
        }
    }
}

// Bit equality is reflexive even for externally constructed non-finite values;
// builds accept finite numbers only and preserve the sign of zero.
impl Eq for ConfigValue {}

impl ConfigValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Number(_) => "number",
            Self::Boolean(_) => "boolean",
        }
    }

    fn token(&self) -> TokenKind {
        match self {
            Self::String(value) => TokenKind::StringLiteral(value.clone()),
            Self::Number(value) => TokenKind::NumberLiteral(*value),
            Self::Boolean(value) => TokenKind::BooleanLiteral(*value),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigParameter {
    pub name: String,
    pub type_name: &'static str,
    pub default: Option<ConfigValue>,
    pub span: Span,
    header: Vec<Token>,
    value_span: Span,
}

pub fn parameters(source: &str, file: FileId) -> Result<Vec<ConfigParameter>, CompileError> {
    let mut asi = Asi::new(source, file);
    let mut tokens = Vec::new();
    loop {
        let token = asi.next_token();
        let eof = token.kind == TokenKind::Eof;
        tokens.push(token);
        if eof {
            break;
        }
    }
    let diagnostics = asi.finish()?;
    if diagnostics
        .iter()
        .any(|diag| diag.severity == Severity::Error)
    {
        return Err(diagnostics.into());
    }
    let mut reader = ConfigReader {
        source,
        tokens: &tokens,
        pos: 0,
    };
    let mut result = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    while reader.peek().kind != TokenKind::Eof {
        if reader.take(&TokenKind::Semicolon) {
            continue;
        }
        let parameter = reader.parameter()?;
        if !names.insert(parameter.name.clone()) {
            return Err(error(
                parameter.span,
                format!("duplicate configuration parameter `{}`", parameter.name),
            ));
        }
        result.push(parameter);
    }
    Ok(result)
}

/// Validate supplied names and types independently of missing required values.
pub fn validate_inputs(
    parameters: &[ConfigParameter],
    inputs: &BTreeMap<String, String>,
    file: FileId,
) -> Result<(), CompileError> {
    for name in inputs.keys() {
        if !parameters.iter().any(|parameter| &parameter.name == name) {
            return Err(error(
                Span::at(file),
                format!(
                    "unknown configuration parameter `{name}`; declare it in src/config.ts or remove the override with --reset-config {name}"
                ),
            ));
        }
    }
    for parameter in parameters {
        if let Some(input) = inputs.get(&parameter.name) {
            parse_input(parameter, input)?;
        }
    }
    Ok(())
}

pub fn resolve(
    parameters: &[ConfigParameter],
    inputs: &BTreeMap<String, String>,
    file: FileId,
) -> Result<BTreeMap<String, ConfigValue>, CompileError> {
    validate_inputs(parameters, inputs, file)?;
    let mut values = BTreeMap::new();
    let mut missing = Vec::new();
    for parameter in parameters {
        let value = if let Some(input) = inputs.get(&parameter.name) {
            Some(parse_input(parameter, input)?)
        } else {
            parameter.default.clone()
        };
        if let Some(value) = value {
            values.insert(parameter.name.clone(), value);
        } else {
            missing.push(Diagnostic {
                severity: Severity::Error,
                span: parameter.span,
                message: format!(
                    "missing required configuration parameter `{}` ({})",
                    parameter.name, parameter.type_name
                ),
                help: vec![format!(
                    "supply --config {}=<{}> for this package",
                    parameter.name, parameter.type_name
                )],
                notes: Vec::new(),
            });
        }
    }
    if !missing.is_empty() {
        return Err(missing.into());
    }
    Ok(values)
}

pub(crate) fn resolved_tokens(
    source: &str,
    file: FileId,
    inputs: &BTreeMap<String, String>,
) -> Result<Vec<Token>, CompileError> {
    let parameters = parameters(source, file)?;
    let values = resolve(&parameters, inputs, file)?;
    let mut tokens = Vec::new();
    for parameter in parameters {
        tokens.extend(parameter.header);
        tokens.push(Token::new(TokenKind::Equals, parameter.value_span));
        let value = values.get(&parameter.name).ok_or_else(|| {
            error(
                parameter.span,
                "configuration resolution omitted a parameter".into(),
            )
        })?;
        tokens.push(Token::new(value.token(), parameter.value_span));
        tokens.push(Token::new(TokenKind::Semicolon, parameter.value_span));
    }
    tokens.push(Token::new(
        TokenKind::Eof,
        Span {
            file,
            start: source.len() as u32,
            end: source.len() as u32,
        },
    ));
    Ok(tokens)
}

fn parse_input(parameter: &ConfigParameter, input: &str) -> Result<ConfigValue, CompileError> {
    let invalid = || {
        error(
            parameter.span,
            format!(
                "configuration parameter `{}` requires {}; supply --config {}=<{}>",
                parameter.name, parameter.type_name, parameter.name, parameter.type_name
            ),
        )
    };
    match parameter.type_name {
        "string" => Ok(ConfigValue::String(string_value::encode(input))),
        "number" => input
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(ConfigValue::Number)
            .ok_or_else(invalid),
        "boolean" => input
            .parse::<bool>()
            .map(ConfigValue::Boolean)
            .map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

fn error(span: Span, message: String) -> CompileError {
    vec![Diagnostic { severity: Severity::Error, span, message, help: vec!["src/config.ts accepts only exported const primitive literals and exported declare const primitive types".into()], notes: Vec::new() }].into()
}

struct ConfigReader<'a> {
    source: &'a str,
    tokens: &'a [Token],
    pos: usize,
}

impl ConfigReader<'_> {
    fn parameter(&mut self) -> Result<ConfigParameter, CompileError> {
        let export = self.expect(&TokenKind::Export)?;
        let declared = self.text(self.peek()) == "declare";
        if declared {
            self.advance();
        }
        let constant = self.expect(&TokenKind::Const)?;
        let name = self.expect(&TokenKind::Identifier)?;
        let mut header = vec![export, constant, name.clone()];
        let annotation = if self.peek().kind == TokenKind::Colon {
            header.push(self.advance());
            let token = self.expect(&TokenKind::Identifier)?;
            let type_name = match self.text(&token) {
                "string" => "string",
                "number" => "number",
                "boolean" => "boolean",
                _ => {
                    return Err(error(
                        token.span,
                        "configuration types must be string, number, or boolean".into(),
                    ));
                }
            };
            header.push(token);
            Some(type_name)
        } else {
            None
        };
        let value_span = self.peek().span;
        let default = if declared {
            if annotation.is_none() {
                return Err(error(
                    name.span,
                    "required configuration needs an explicit primitive type".into(),
                ));
            }
            None
        } else {
            self.expect(&TokenKind::Equals)?;
            let negative = self.take(&TokenKind::Minus);
            let literal = self.advance();
            let value = match literal.kind {
                TokenKind::StringLiteral(value) | TokenKind::TemplateNoSubstitution(value)
                    if !negative =>
                {
                    ConfigValue::String(value)
                }
                TokenKind::NumberLiteral(value) if value.is_finite() => {
                    ConfigValue::Number(if negative { -value } else { value })
                }
                TokenKind::BooleanLiteral(value) if !negative => ConfigValue::Boolean(value),
                _ => {
                    return Err(error(
                        literal.span,
                        "configuration defaults must be primitive literals".into(),
                    ));
                }
            };
            if annotation.is_some_and(|name| name != value.type_name()) {
                return Err(error(
                    literal.span,
                    "configuration default does not match its declared type".into(),
                ));
            }
            Some(value)
        };
        if self.peek().kind != TokenKind::Eof {
            self.expect(&TokenKind::Semicolon)?;
        }
        let type_name = annotation
            .or_else(|| default.as_ref().map(ConfigValue::type_name))
            .ok_or_else(|| {
                error(
                    name.span,
                    "configuration parameter needs a type or default".into(),
                )
            })?;
        Ok(ConfigParameter {
            name: self.text(&name).to_owned(),
            type_name,
            default,
            span: name.span,
            header,
            value_span,
        })
    }

    fn peek(&self) -> &Token {
        // The lexer always appends EOF, and advance never advances past it.
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if token.kind != TokenKind::Eof {
            self.pos += 1;
        }
        token
    }

    fn take(&mut self, kind: &TokenKind) -> bool {
        if &self.peek().kind != kind {
            return false;
        }
        self.advance();
        true
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<Token, CompileError> {
        if &self.peek().kind != kind {
            return Err(error(
                self.peek().span,
                format!("invalid configuration declaration: expected {kind:?}"),
            ));
        }
        Ok(self.advance())
    }

    fn text(&self, token: &Token) -> &str {
        // Token spans are produced by Asi from this source and are not changed.
        &self.source[token.span.start as usize..token.span.end as usize]
    }
}

/// Metadata uses ordinary JSON strings where possible, with an explicit UTF-16
/// representation only when a default contains an unpaired surrogate.
mod string_value {
    use crate::literal_units::{literal_units, push_literal_char, push_lone_surrogate};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Text(String),
        Units { utf16: Vec<u16> },
    }

    pub fn encode(text: &str) -> String {
        let mut literal = String::new();
        for character in text.chars() {
            push_literal_char(&mut literal, character);
        }
        literal
    }

    pub fn serialize<S: Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        let units = literal_units(text);
        match String::from_utf16(&units) {
            Ok(text) => Wire::Text(text).serialize(serializer),
            Err(_) => Wire::Units { utf16: units }.serialize(serializer),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        match Wire::deserialize(deserializer)? {
            Wire::Text(text) => Ok(encode(&text)),
            Wire::Units { utf16 } => {
                let mut literal = String::new();
                for character in char::decode_utf16(utf16) {
                    match character {
                        Ok(character) => push_literal_char(&mut literal, character),
                        Err(error) => {
                            push_lone_surrogate(
                                &mut literal,
                                u32::from(error.unpaired_surrogate()),
                            );
                        }
                    }
                }
                Ok(literal)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_required_values_and_primitive_validation() {
        let source = "// Configuration\nexport declare const url: string;\nexport const timeout = -5000;\nexport const enabled: boolean = false;\nexport const title = `hello`;";
        let parameters = parameters(source, FileId(0)).unwrap();
        assert!(resolve(&parameters, &BTreeMap::new(), FileId(0)).is_err());
        let inputs = BTreeMap::from([
            ("url".into(), "https://example.test/?a=b".into()),
            ("timeout".into(), "0.5".into()),
            ("enabled".into(), "true".into()),
        ]);
        let values = resolve(&parameters, &inputs, FileId(0)).unwrap();
        assert_eq!(
            values["url"],
            ConfigValue::String("https://example.test/?a=b".into())
        );
        assert_eq!(values["enabled"], ConfigValue::Boolean(true));
        let tokens = resolved_tokens(source, FileId(0), &inputs).unwrap();
        let (_, diagnostics) = crate::parser::parse_checked(source, tokens, FileId(0)).unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        for (name, value) in [
            ("timeout", "NaN"),
            ("timeout", "inf"),
            ("enabled", "yes"),
            ("unknown", "value"),
        ] {
            let mut invalid = inputs.clone();
            invalid.insert(name.into(), value.into());
            assert!(
                resolve(&parameters, &invalid, FileId(0)).is_err(),
                "{name}={value}"
            );
        }
    }

    #[test]
    fn rejects_executable_configuration_and_conflicting_declarations() {
        for source in [
            "export let x = 1;",
            "const x = 1;",
            "export const x = 1 + 2;",
            "export const x = Math.random();",
            "export const x = {};",
            "export declare const x;",
            "export declare const x: string | undefined;",
            "export const x: number = 'text';",
            "export const x = 1; export const x = 2;",
            "import { x } from './other';",
            "export function x(): number { return 1; }",
        ] {
            assert!(parameters(source, FileId(0)).is_err(), "{source}");
        }
    }
}

#[cfg(test)]
mod representation_tests {
    use super::*;

    #[test]
    fn signed_zero_survives_defaults_overrides_and_metadata() {
        let parameters = parameters("export const value = -0;", FileId(0)).unwrap();
        for inputs in [
            BTreeMap::new(),
            BTreeMap::from([("value".into(), "-0".into())]),
        ] {
            let values = resolve(&parameters, &inputs, FileId(0)).unwrap();
            assert_eq!(values["value"], ConfigValue::Number(-0.0));
            assert_ne!(values["value"], ConfigValue::Number(0.0));
            let text = serde_json::to_string(&values).unwrap();
            let decoded: BTreeMap<String, ConfigValue> = serde_json::from_str(&text).unwrap();
            assert_eq!(decoded, values);
        }
    }

    #[test]
    fn marker_characters_and_lone_surrogates_roundtrip_without_loss() {
        let source = "export const text = '\\ud800';";
        let parameters = parameters(source, FileId(0)).unwrap();
        let default = resolve(&parameters, &BTreeMap::new(), FileId(0)).unwrap();
        let encoded = serde_json::to_string(&default).unwrap();
        assert!(encoded.contains("utf16"));
        let decoded: BTreeMap<String, ConfigValue> = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, default);
        let input = "\u{10fffe}\u{100000}";
        let values = resolve(
            &parameters,
            &BTreeMap::from([("text".into(), input.into())]),
            FileId(0),
        )
        .unwrap();
        let ConfigValue::String(text) = &values["text"] else {
            panic!("string");
        };
        assert_eq!(
            crate::literal_units::literal_units(text),
            input.encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(serde_json::to_value(&values).unwrap()["text"], input);
        let decoded: BTreeMap<String, ConfigValue> =
            serde_json::from_value(serde_json::to_value(&values).unwrap()).unwrap();
        assert_eq!(decoded, values);
    }
}
