//! x87 instruction forms share one catalog entry point.

mod arithmetic;
mod compare;
mod conditional_move;
mod control;
mod load;
mod stack;
mod store;

use super::*;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    control::forms()
        .chain(arithmetic::forms())
        .chain(compare::forms())
        .chain(conditional_move::forms())
        .chain(stack::forms())
        .chain(load::forms())
        .chain(store::forms())
}
