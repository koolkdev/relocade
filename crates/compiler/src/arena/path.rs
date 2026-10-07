//! Decisions already made on the active construction path.
use std::rc::Rc;

use super::FunctionArena;
use crate::{
    body::{ValueDefinition, ValueTable},
    BuildError, Expression,
};

/// Branches share their inherited decisions. Discarding a child also discards
/// its refinements, without changing siblings or path-independent value interning.
#[derive(Clone, Default)]
pub(crate) struct Path(Option<Rc<Decision>>);

struct Decision {
    value: usize,
    nonzero: bool,
    parent: Path,
}

impl FunctionArena {
    pub(crate) fn condition(
        &self,
        path: &Path,
        condition: usize,
    ) -> Result<Option<bool>, BuildError> {
        self.with_graph(|graph| {
            let (value, nonzero) = predicate(&graph.values, condition);
            if let ValueDefinition::Constant(bits) = graph.values[value].definition {
                return Some((bits != 0) == nonzero);
            }
            let mut current = &path.0;
            while let Some(decision) = current {
                if decision.value == value {
                    return Some(decision.nonzero == nonzero);
                }
                current = &decision.parent.0;
            }
            None
        })
    }

    pub(crate) fn assume(
        &self,
        path: &Path,
        condition: usize,
        truth: bool,
    ) -> Result<Path, BuildError> {
        self.with_graph(|graph| {
            let (value, nonzero) = predicate(&graph.values, condition);
            Path(Some(Rc::new(Decision {
                value,
                nonzero: nonzero == truth,
                parent: path.clone(),
            })))
        })
    }
}

/// A normalized condition tests a carrier for zero. Strip inversions and
/// carrier-preserving views so a guard and its opposite refer to one decision.
/// Logical narrowing keeps its explicit mask and therefore a distinct value.
fn predicate(values: &ValueTable, mut value: usize) -> (usize, bool) {
    let mut nonzero = true;
    loop {
        value = values.representation(value);
        match values[value].definition {
            ValueDefinition::Expression(Expression::ZeroTest {
                input,
                nonzero: test,
            }) => {
                value = input;
                nonzero = nonzero == test;
            }
            _ => return (value, nonzero),
        }
    }
}
