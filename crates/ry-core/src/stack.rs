//! On-demand stack growth for recursion the syntax nesting limit does not
//! bound.
//!
//! An `else if` arm keeps its parent's syntax depth (R code writes the chain
//! flat), but lowering, inference, and cloning still recurse once per arm,
//! at several KiB a step in debug builds. Those recursions call [`grow`] at
//! their entry points, so a long chain, or deep code inside one, moves to a
//! heap-allocated stack segment instead of overflowing a 2 MiB worker.

/// Stack that must remain before a step runs on the current segment. It
/// covers the frames between two `grow` calls.
const RED_ZONE: usize = 256 * 1024;

/// Size of each segment added when less than [`RED_ZONE`] remains.
const STACK_PER_GROW: usize = 2 * 1024 * 1024;

/// Run `f`, first switching to a fresh stack segment when the current one
/// is nearly used up.
pub fn grow<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, STACK_PER_GROW, f)
}
