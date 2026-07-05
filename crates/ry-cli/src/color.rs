//! `--color` handling.
//!
//! The CLI does not currently emit any colorized output: the human
//! formats (`concise`, `full`) are plain text. To stop advertising a
//! no-op flag while staying forward-compatible, `--color` is now PARSED
//! and VALIDATED (an unknown value is a hard error) and `NO_COLOR` is
//! honored, but the resolved choice has no observable effect today --
//! every variant yields plain text. When coloring is added, the only
//! change needed is to gate emission on `ColorChoice::should_color()`.

/// The resolved color preference. `Auto` defers to the ambient
/// environment (`NO_COLOR` => off, otherwise on); `Always`/`Never` are
/// explicit overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    /// Parse a `--color=WHEN` value. Recognizes `auto`, `always`,
    /// `never` (case-insensitive). Returns `Err` with a helpful message
    /// for anything else so clap users get a clear error.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(ColorChoice::Auto),
            "always" => Ok(ColorChoice::Always),
            "never" => Ok(ColorChoice::Never),
            other => Err(format!(
                "unknown `--color` value `{other}`; expected one of: auto, always, never"
            )),
        }
    }

    /// Resolve the effective setting, honoring `NO_COLOR` (any non-empty
    /// value disables color). Returns whether color should be applied.
    ///
    /// Today the CLI emits no colorized output, so callers can consult
    /// this for forward compatibility but it does not change rendering.
    #[allow(dead_code)] // no colorized output paths exist yet
    pub fn should_color(self) -> bool {
        if std::env::var_os("NO_COLOR")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        {
            return false;
        }
        match self {
            ColorChoice::Never => false,
            ColorChoice::Always | ColorChoice::Auto => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_variants() {
        assert_eq!(ColorChoice::parse("auto").unwrap(), ColorChoice::Auto);
        assert_eq!(ColorChoice::parse("ALWAYS").unwrap(), ColorChoice::Always);
        assert_eq!(ColorChoice::parse("Never").unwrap(), ColorChoice::Never);
    }

    #[test]
    fn rejects_unknown() {
        assert!(ColorChoice::parse("yes").is_err());
        assert!(ColorChoice::parse("").is_err());
    }

    #[test]
    fn never_disables_color_regardless_of_no_color() {
        // NO_COLOR unset (in this test process it may or may not be set;
        // Never wins regardless).
        assert!(!ColorChoice::Never.should_color());
    }
}
