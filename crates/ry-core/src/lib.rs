//! ry-core: parsing, AST, and the R type lattice.
//! Does NOT depend on the checker; the checker depends on this.

pub mod ast;
pub mod parser;
pub mod span;
pub mod types;

pub use ast::*;
pub use parser::{ParseError, RParser};
pub use span::Span;
pub use tree_sitter::{InputEdit, Point, Tree};
pub use types::{Length, Mode, RType};

pub mod diagnostic;
pub use diagnostic::{
    BaselineDiagnostic, Confidence, FFI_PRIMITIVES, SERIALIZED_BINDINGS_UNENUMERABLE, Severity,
};
