use std::fmt::{self, Write};
use std::io;
use std::time::SystemTime;

use serde_json::Value;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::{
    FmtContext, FormattedFields,
    format::{FormatEvent, FormatFields, Writer},
};
use tracing_subscriber::registry::LookupSpan;

const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_STRUCTURED_NODES: usize = 4096;
const HEADER_KEYS: [&str; 5] = ["ts", "level", "stream", "target", "msg"];
// Encoded values never contain a raw NUL, so this cache marker cannot be
// confused with a real field. Invalid spans reject the entire event.
const INVALID_SPAN_FIELDS: &str = "\0";

/// Routing discriminator shared by diagnostic logs and future audit records.
#[derive(Clone, Copy)]
pub enum Stream {
    Log,
    Audit,
}

/// Exact logfmt event headers. A fixed timestamp supports deterministic tests.
#[derive(Default)]
pub struct Logfmt {
    pub timestamp: Option<jiff::Timestamp>,
}

impl<S, N> FormatEvent<S, N> for Logfmt
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        match self.format_line(ctx, event) {
            Ok(line) => writer.write_str(&line),
            Err(error) => {
                super::output::report(
                    "cannot format server log: invalid field, timestamp, or record exceeds 1 MiB",
                );
                Err(error)
            }
        }
    }
}

impl Logfmt {
    fn format_line<S, N>(
        &self,
        ctx: &FmtContext<'_, S, N>,
        event: &Event<'_>,
    ) -> Result<String, fmt::Error>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
        N: for<'a> FormatFields<'a> + 'static,
    {
        let timestamp = match self.timestamp {
            Some(timestamp) => timestamp,
            None => jiff::Timestamp::try_from(SystemTime::now()).map_err(|_| fmt::Error)?,
        };
        let mut fields = Fields::default();
        event.record(&mut fields);
        fields.result?;
        let metadata = event.metadata();
        let mut line = header(
            timestamp,
            metadata.level(),
            Stream::Log,
            metadata.target(),
            &fields.message,
        )?;
        line.write_str(&fields.values.text)?;
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let extensions = span.extensions();
                if let Some(fields) = extensions.get::<FormattedFields<N>>() {
                    if fields.contains(INVALID_SPAN_FIELDS) {
                        return Err(fmt::Error);
                    }
                    line.write_str(fields)?;
                }
            }
        }
        line.write_char('\n')?;
        Ok(line.text)
    }
}

/// Span fields use the same escaping and reserved-key handling as events.
pub struct LogfmtFields;

impl<'a> FormatFields<'a> for LogfmtFields {
    fn format_fields<R: RecordFields>(&self, mut writer: Writer<'a>, fields: R) -> fmt::Result {
        let mut visitor = Fields {
            span: true,
            ..Fields::default()
        };
        fields.record(&mut visitor);
        writer.write_str(&span_fields(visitor))
    }

    fn add_fields(
        &self,
        current: &'a mut FormattedFields<Self>,
        fields: &tracing::span::Record<'_>,
    ) -> fmt::Result {
        let mut visitor = Fields {
            span: true,
            ..Fields::default()
        };
        if current.fields.contains(INVALID_SPAN_FIELDS) {
            return Ok(());
        }
        visitor.result = visitor.values.write_str(&current.fields);
        fields.record(&mut visitor);
        current.fields = span_fields(visitor);
        Ok(())
    }
}

fn span_fields(visitor: Fields) -> String {
    if visitor.result.is_err() {
        // tracing-subscriber prints rejected attributes with eprintln! if this
        // formatter returns Err. Report safely and cache an explicit failure
        // instead; never emit a successful event with incomplete span context.
        super::output::report(
            "cannot format server log span: invalid field or fields exceed 1 MiB",
        );
        INVALID_SPAN_FIELDS.to_owned()
    } else {
        visitor.values.text
    }
}

/// Encode structured fields without parsing tracing debug strings. Objects and
/// lists flatten to dotted paths; traversal borrows values and bounds depth.
pub fn encode_record(
    timestamp: jiff::Timestamp,
    level: &tracing::Level,
    stream: Stream,
    target: &str,
    message: &str,
    fields: &serde_json::Map<String, Value>,
) -> io::Result<String> {
    let encode = || -> Result<String, fmt::Error> {
        let mut line = header(timestamp, level, stream, target, message)?;
        let mut remaining_nodes = MAX_STRUCTURED_NODES;
        for (key, value) in fields {
            validate_key(key)?;
            flatten(&mut line, field_key(key), value, 0, &mut remaining_nodes)?;
        }
        line.write_char('\n')?;
        Ok(line.text)
    };
    encode().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid logfmt key, excessive nesting/fields, or record exceeds 1 MiB",
        )
    })
}

fn flatten(
    line: &mut Buffer,
    key: String,
    value: &Value,
    depth: usize,
    remaining_nodes: &mut usize,
) -> fmt::Result {
    *remaining_nodes = remaining_nodes.checked_sub(1).ok_or(fmt::Error)?;
    validate_key(&key)?;
    if depth >= MAX_DEPTH || key.len() > MAX_RECORD_BYTES {
        return Err(fmt::Error);
    }
    match value {
        Value::Object(fields) => {
            for (child, value) in fields {
                validate_key(child)?;
                flatten(
                    line,
                    format!("{key}.{child}"),
                    value,
                    depth + 1,
                    remaining_nodes,
                )?;
            }
            Ok(())
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                flatten(
                    line,
                    format!("{key}.{index}"),
                    value,
                    depth + 1,
                    remaining_nodes,
                )?;
            }
            Ok(())
        }
        Value::String(value) => entry(line, &key, value),
        _ => entry(line, &key, &value.to_string()),
    }
}

fn header(
    timestamp: jiff::Timestamp,
    level: &tracing::Level,
    stream: Stream,
    target: &str,
    message: &str,
) -> Result<Buffer, fmt::Error> {
    let mut line = Buffer::default();
    write!(
        line,
        "ts={timestamp:.3} level={}",
        match *level {
            tracing::Level::ERROR => "error",
            tracing::Level::WARN => "warn",
            tracing::Level::INFO => "info",
            tracing::Level::DEBUG => "debug",
            tracing::Level::TRACE => "trace",
        }
    )?;
    entry(
        &mut line,
        "stream",
        match stream {
            Stream::Log => "log",
            Stream::Audit => "audit",
        },
    )?;
    entry(&mut line, "target", target)?;
    entry(&mut line, "msg", message)?;
    Ok(line)
}

fn entry(line: &mut Buffer, key: &str, value: &str) -> fmt::Result {
    validate_key(key)?;
    if value.len() > MAX_RECORD_BYTES {
        return Err(fmt::Error);
    }
    write!(line, " {key}=")?;
    let quoted = value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '=' | '"'));
    if !quoted {
        return line.write_str(value);
    }
    line.write_char('"')?;
    for c in value.chars() {
        match c {
            '"' => line.write_str("\\\"")?,
            '\\' => line.write_str("\\\\")?,
            '\n' => line.write_str("\\n")?,
            '\r' => line.write_str("\\r")?,
            '\t' => line.write_str("\\t")?,
            c if c.is_control() => write!(line, "\\u{:04x}", u32::from(c))?,
            c => line.write_char(c)?,
        }
    }
    line.write_char('"')
}

fn validate_key(key: &str) -> fmt::Result {
    if key.is_empty()
        || key.len() > MAX_RECORD_BYTES
        || key
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '=' | '"' | '\\'))
    {
        return Err(fmt::Error);
    }
    Ok(())
}

fn field_key(key: &str) -> String {
    if HEADER_KEYS.contains(&key) {
        format!("fields.{key}")
    } else {
        key.to_owned()
    }
}

struct Fields {
    message: String,
    values: Buffer,
    result: fmt::Result,
    span: bool,
}

impl Default for Fields {
    fn default() -> Self {
        Self {
            message: String::new(),
            values: Buffer::default(),
            result: Ok(()),
            span: false,
        }
    }
}

impl Fields {
    fn record(&mut self, field: &Field, value: &str) {
        if self.result.is_err() {
            return;
        }
        if value.len() > MAX_RECORD_BYTES {
            self.result = Err(fmt::Error);
            return;
        }
        if field.name() == "message" && !self.span {
            self.message = value.to_owned();
        } else {
            self.result = entry(&mut self.values, &field_key(field.name()), value);
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record(field, value);
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if self.result.is_err() {
            return;
        }
        let mut text = Buffer::default();
        match write!(text, "{value:?}") {
            Ok(()) => self.record(field, &text.text),
            Err(error) => self.result = Err(error),
        }
    }
}

#[derive(Default)]
struct Buffer {
    text: String,
}

impl Write for Buffer {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let size = self.text.len().checked_add(value.len()).ok_or(fmt::Error)?;
        if size > MAX_RECORD_BYTES {
            return Err(fmt::Error);
        }
        self.text.try_reserve(value.len()).map_err(|_| fmt::Error)?;
        self.text.push_str(value);
        Ok(())
    }
}
