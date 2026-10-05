//! Bounded diagnostic output. Truncation is presentation, not a compiler failure.

use std::fmt;

pub const TRUNCATED: &str = "[diagnostic output truncated]";

#[derive(Clone, Copy, Debug)]
pub struct RenderLimits {
    pub bytes: usize,
    pub steps: usize,
    pub type_depth: usize,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            bytes: 64 * 1024,
            steps: 65_536,
            type_depth: 512,
        }
    }
}

impl RenderLimits {
    pub fn collection() -> Self {
        Self {
            bytes: 1024 * 1024,
            ..Self::default()
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RenderedText {
    pub text: String,
    pub truncated: bool,
    steps: usize,
}

#[derive(Debug)]
pub enum RenderError {
    InvalidMetadata(&'static str),
    Source(crate::source::SourceError),
    Allocation,
    Formatting,
    /// Private rendering control flow, consumed by `Writer::render`.
    Truncated,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMetadata(context) => write!(f, "invalid diagnostic metadata: {context}"),
            Self::Source(error) => write!(
                f,
                "invalid diagnostic metadata; source context unavailable: {error}"
            ),
            Self::Allocation => f.write_str("diagnostic allocation failed"),
            Self::Formatting => f.write_str("diagnostic formatting failed"),
            Self::Truncated => f.write_str(TRUNCATED),
        }
    }
}
impl std::error::Error for RenderError {}
impl From<crate::source::SourceError> for RenderError {
    fn from(error: crate::source::SourceError) -> Self {
        Self::Source(error)
    }
}
impl From<RenderError> for crate::compiler_error::CompilerFailure {
    fn from(error: RenderError) -> Self {
        Self::Internal {
            stage: crate::compiler_error::CompilerStage::Infer,
            span: None,
            message: error.to_string(),
        }
    }
}

pub(crate) struct Writer {
    text: String,
    limits: RenderLimits,
    steps: usize,
    failure: Option<RenderError>,
    abbreviated: bool,
}

impl RenderedText {
    pub(crate) fn steps(&self) -> usize {
        self.steps
    }
}

impl Writer {
    pub(crate) fn remaining_steps(&self) -> usize {
        self.limits.steps.saturating_sub(self.steps)
    }

    pub(crate) fn render(
        limits: RenderLimits,
        render: impl FnOnce(&mut Self) -> Result<(), RenderError>,
    ) -> Result<RenderedText, RenderError> {
        if limits.bytes < TRUNCATED.len() {
            return Err(RenderError::InvalidMetadata(
                "render byte limit cannot hold truncation marker",
            ));
        }
        let mut out = Self {
            text: String::new(),
            limits,
            steps: 0,
            failure: None,
            abbreviated: false,
        };
        let result = render(&mut out);
        match out.failure.take().map_or(result, Err) {
            Ok(()) => Ok(RenderedText {
                text: out.text,
                truncated: out.abbreviated,
                steps: out.steps,
            }),
            Err(RenderError::Truncated) => {
                let budget = limits.bytes.saturating_sub(TRUNCATED.len());
                let mut end = out.text.len().min(budget);
                while !out.text.is_char_boundary(end) {
                    end = end.saturating_sub(1);
                }
                out.text.truncate(end);
                // Very small caller limits still produce a complete, unambiguous marker.
                out.text
                    .try_reserve(TRUNCATED.len())
                    .map_err(|_| RenderError::Allocation)?;
                out.text.push_str(TRUNCATED);
                Ok(RenderedText {
                    text: out.text,
                    truncated: true,
                    steps: out.steps,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn append(&mut self, rendered: RenderedText) -> Result<(), RenderError> {
        self.abbreviated |= rendered.truncated;
        self.steps = self
            .steps
            .checked_add(rendered.steps)
            .ok_or(RenderError::Truncated)?;
        self.push(&rendered.text)
    }

    pub(crate) fn child_limits(&self) -> RenderLimits {
        RenderLimits {
            bytes: self
                .limits
                .bytes
                .saturating_sub(self.text.len())
                .max(TRUNCATED.len())
                .min(RenderLimits::default().bytes),
            steps: self
                .limits
                .steps
                .saturating_sub(self.steps)
                .saturating_sub(1),
            type_depth: self.limits.type_depth,
        }
    }

    pub(crate) fn step(&mut self) -> Result<(), RenderError> {
        if self.steps >= self.limits.steps {
            return Err(RenderError::Truncated);
        }
        self.steps += 1;
        Ok(())
    }

    pub(crate) fn depth(&self, depth: usize) -> Result<(), RenderError> {
        if depth > self.limits.type_depth {
            Err(RenderError::Truncated)
        } else {
            Ok(())
        }
    }

    pub(crate) fn push(&mut self, text: &str) -> Result<(), RenderError> {
        self.step()?;
        let remaining = self.limits.bytes.saturating_sub(self.text.len());
        let mut end = remaining.min(text.len());
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        let prefix = text.get(..end).ok_or(RenderError::Formatting)?;
        self.text
            .try_reserve(prefix.len())
            .map_err(|_| RenderError::Allocation)?;
        self.text.push_str(prefix);
        if end < text.len() {
            return Err(RenderError::Truncated);
        }
        Ok(())
    }

    pub(crate) fn character(&mut self, ch: char) -> Result<(), RenderError> {
        self.push(ch.encode_utf8(&mut [0; 4]))
    }

    pub(crate) fn format(&mut self, args: fmt::Arguments<'_>) -> Result<(), RenderError> {
        match fmt::write(self, args) {
            Ok(()) => Ok(()),
            Err(_) => Err(self.failure.take().unwrap_or(RenderError::Formatting)),
        }
    }

    pub(crate) fn repeat(&mut self, ch: char, count: usize) -> Result<(), RenderError> {
        for _ in 0..count {
            self.character(ch)?;
        }
        Ok(())
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.push(text).map_err(|error| {
            self.failure = Some(error);
            fmt::Error
        })
    }
}

/// Source-less compatibility text; never re-enters the failed renderer.
pub fn failure_text(primary: &str, error: &RenderError) -> String {
    match Writer::render(RenderLimits::default(), |out| {
        // Put the reporting failure first so a large primary cannot hide it.
        out.format(format_args!("internal reporting failure: {error}\n"))?;
        out.push(primary)
    }) {
        Ok(rendered) => rendered.text,
        Err(_) => "internal reporting failure: diagnostic output unavailable".into(),
    }
}

/// Bound a retained diagnostic field before copying it into a response.
pub fn bounded_text(text: &str, bytes: usize) -> Result<RenderedText, RenderError> {
    Writer::render(
        RenderLimits {
            bytes,
            ..RenderLimits::default()
        },
        |out| out.push(text),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_boundaries_are_exact_and_unicode_truncation_is_valid() {
        for extra in [0, 1] {
            let input = "é".repeat(32 + extra);
            let rendered = bounded_text(&input, 64).unwrap();
            assert!(rendered.text.len() <= 64);
            assert_eq!(rendered.truncated, extra != 0);
            if extra == 0 {
                assert_eq!(rendered.text, input);
            } else {
                assert!(rendered.text.ends_with(TRUNCATED));
            }
        }
    }

    #[test]
    fn work_limit_and_allocation_failure_do_not_publish_partial_success() {
        let limits = RenderLimits {
            steps: 2,
            ..Default::default()
        };
        let exact = Writer::render(limits, |out| {
            out.push("first")?;
            out.push("second")
        })
        .unwrap();
        assert!(!exact.truncated);
        let exceeded = Writer::render(limits, |out| {
            out.push("first")?;
            out.push("second")?;
            out.push("third")
        })
        .unwrap();
        assert!(exceeded.truncated);
        let failed = Writer::render(limits, |out| {
            out.push("partial")?;
            Err(RenderError::Allocation)
        });
        assert!(matches!(failed, Err(RenderError::Allocation)));
        assert_eq!(
            Writer::render(limits, |out| out.push("healthy"))
                .unwrap()
                .text,
            "healthy"
        );
    }

    #[test]
    fn a_broken_display_is_a_typed_failure() {
        struct Broken;
        impl fmt::Display for Broken {
            fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
                Err(fmt::Error)
            }
        }
        let result = Writer::render(RenderLimits::default(), |out| {
            out.format(format_args!("{Broken}"))
        });
        assert!(matches!(result, Err(RenderError::Formatting)));
    }
}
