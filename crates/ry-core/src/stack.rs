//! On-demand stack growth for recursion the syntax nesting limit does not
//! bound.
//!
//! An `else if` arm keeps its parent's syntax depth (R code writes the chain
//! flat), but the lowering and inference walks still recurse once per arm,
//! at several KiB a step. Those walks call [`grow`] so a long chain moves to
//! a heap-allocated stack segment instead of overflowing a 2 MiB worker.

/// Stack that must remain before a step runs on the current segment. It
/// covers one arm's recursion plus ordinary nested work inside the arm.
const RED_ZONE: usize = 256 * 1024;

/// Size of each segment added when less than [`RED_ZONE`] remains.
const STACK_PER_GROW: usize = 2 * 1024 * 1024;

/// Run `f`, first switching to a fresh stack segment when the current one
/// is nearly used up.
pub fn grow<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, STACK_PER_GROW, f)
}
