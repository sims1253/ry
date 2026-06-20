//! ry-core: parsing, AST, and the R type lattice.
//! Does NOT depend on the checker; the checker depends on this.

pub mod ast;
pub mod parser;
pub mod span;
pub mod types;

pub use ast::*;
pub use parser::{ParseError, RParser};
pub use span::Span;
pub use types::{Length, Mode, NaFlag, RType};
