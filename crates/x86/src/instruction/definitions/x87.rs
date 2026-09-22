//! x87 instruction forms share one catalog entry point.

mod control;
mod stack;

use super::*;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    control::forms().chain(stack::forms())
}
