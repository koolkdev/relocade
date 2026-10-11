//! SSE and SSE2 instruction families share one catalog entry point.

mod half_moves;
mod logical;
mod moves;
mod mxcsr;

use super::*;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    moves::forms()
        .chain(half_moves::forms())
        .chain(logical::forms())
        .chain(mxcsr::forms())
}
