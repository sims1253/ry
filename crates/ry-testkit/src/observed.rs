use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::{Component, Path};

use crate::FixtureProject;

pub type DriverError = Box<dyn Error + Send + Sync + 'static>;

/// An owning-crate adapter that observes diagnostics at a published seam.
pub trait Driver {
    fn published_diagnostics(
        &mut self,
        fixture: &FixtureProject,
    ) -> Result<Vec<ObservedDiagnostic>, DriverError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionEncoding {
    /// A byte column within a UTF-8 source line.
    Utf8Byte,
    /// A Unicode scalar-value column, as emitted by the CLI.
    UnicodeScalar,
    /// A UTF-16 code-unit column, as required by LSP.
    Utf16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedPosition {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based column in [`Self::encoding`].
    pub character: u32,
    pub encoding: PositionEncoding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedRange {
    pub start: ObservedPosition,
    pub end: Option<ObservedPosition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedFix {
    pub range: ObservedRange,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedDiagnostic {
    pub path: String,
    pub code: String,
    pub severity: String,
    pub message: String,
    pub range: ObservedRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<ObservedFix>,
}

/// Make an observed path stable without resolving symlinks or requiring it to exist.
/// Absolute paths below `root` become root-relative; separators are `/` on all hosts.
pub fn normalize_path(path: impl AsRef<Path>, root: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    let root = root.as_ref();
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().is_some_and(|part| part != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::RootDir => {}
            Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().into_owned())
            }
        }
    }
    parts.join("/")
}

/// Convert a zero-based source position to a UTF-8 byte column.
///
/// This is intentionally only an encoding conversion. It does not alter code,
/// severity, message, or any other diagnostic behavior.
pub fn normalize_position(
    source: &str,
    position: &ObservedPosition,
) -> Result<ObservedPosition, DriverError> {
    let line = source
        .split('\n')
        .nth(position.line as usize)
        .ok_or_else(|| format!("line {} is outside source", position.line))?;
    let byte_column = match position.encoding {
        PositionEncoding::Utf8Byte => {
            let column = position.character as usize;
            if column > line.len() || !line.is_char_boundary(column) {
                return Err(format!("byte column {column} is not a source boundary").into());
            }
            column
        }
        PositionEncoding::UnicodeScalar => scalar_to_byte(line, position.character as usize)?,
        PositionEncoding::Utf16 => utf16_to_byte(line, position.character as usize)?,
    };
    Ok(ObservedPosition {
        line: position.line,
        character: byte_column as u32,
        encoding: PositionEncoding::Utf8Byte,
    })
}

fn scalar_to_byte(line: &str, column: usize) -> Result<usize, DriverError> {
    if column == line.chars().count() {
        return Ok(line.len());
    }
    line.char_indices()
        .nth(column)
        .map(|(offset, _)| offset)
        .ok_or_else(|| format!("scalar column {column} is outside source line").into())
}

fn utf16_to_byte(line: &str, column: usize) -> Result<usize, DriverError> {
    let mut utf16 = 0;
    for (offset, character) in line.char_indices() {
        if utf16 == column {
            return Ok(offset);
        }
        utf16 += character.len_utf16();
        if utf16 > column {
            return Err(format!("UTF-16 column {column} splits a surrogate pair").into());
        }
    }
    if utf16 == column {
        Ok(line.len())
    } else {
        Err(format!("UTF-16 column {column} is outside source line").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_scalar_and_utf16_columns_independently() {
        let source = "aé😀z\n";
        let scalar = ObservedPosition {
            line: 0,
            character: 3,
            encoding: PositionEncoding::UnicodeScalar,
        };
        let utf16 = ObservedPosition {
            line: 0,
            character: 4,
            encoding: PositionEncoding::Utf16,
        };
        assert_eq!(normalize_position(source, &scalar).unwrap().character, 7);
        assert_eq!(normalize_position(source, &utf16).unwrap().character, 7);
    }

    #[test]
    fn rejects_a_position_inside_an_astral_surrogate_pair() {
        let position = ObservedPosition {
            line: 0,
            character: 1,
            encoding: PositionEncoding::Utf16,
        };
        assert!(normalize_position("😀", &position).is_err());
    }

    #[test]
    fn paths_below_root_are_stable_and_relative() {
        assert_eq!(
            normalize_path("/tmp/project/R/x.R", "/tmp/project"),
            "R/x.R"
        );
    }
}
