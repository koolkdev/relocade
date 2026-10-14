//! x87 instruction forms share one catalog entry point.

mod arctangent;
mod arithmetic;
mod compare;
mod conditional_move;
mod control;
mod environment;
mod examine;
mod exponential;
mod extract;
mod integral;
mod load;
mod logarithm;
mod remainder;
mod result;
mod scale;
mod sign;
mod sqrt;
mod stack;
mod store;

use super::*;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    control::forms()
        .chain(environment::forms())
        .chain(arithmetic::forms())
        .chain(arctangent::forms())
        .chain(compare::forms())
        .chain(conditional_move::forms())
        .chain(examine::forms())
        .chain(exponential::forms())
        .chain(extract::forms())
        .chain(sqrt::forms())
        .chain(integral::forms())
        .chain(stack::forms())
        .chain(load::forms())
        .chain(logarithm::forms())
        .chain(remainder::forms())
        .chain(scale::forms())
        .chain(sign::forms())
        .chain(store::forms())
}
