//! x87 instruction forms share one catalog entry point.

mod arithmetic;
mod compare;
mod conditional_move;
mod control;
mod examine;
mod extract;
mod integral;
mod load;
mod result;
mod scale;
mod sign;
mod sqrt;
mod stack;
mod store;

use super::*;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    control::forms()
        .chain(arithmetic::forms())
        .chain(compare::forms())
        .chain(conditional_move::forms())
        .chain(examine::forms())
        .chain(extract::forms())
        .chain(sqrt::forms())
        .chain(integral::forms())
        .chain(stack::forms())
        .chain(load::forms())
        .chain(scale::forms())
        .chain(sign::forms())
        .chain(store::forms())
}
