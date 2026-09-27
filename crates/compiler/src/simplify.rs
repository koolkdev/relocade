//! Simplification of completed construction before effects and placement.
use crate::body::{Block, Body, ValueTable};

mod control;
mod paths;

pub(super) fn body(mut values: ValueTable, mut block: Block) -> Body {
    paths::simplify(&mut values, &mut block);
    control::fold(&mut block, &values.values);
    Body {
        values: values.values,
        block,
    }
}
