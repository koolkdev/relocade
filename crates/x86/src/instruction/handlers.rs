//! Callable instruction shapes shared by decoding and lowering.
//!
//! Every operand shape receives the bound condition and fallthrough EIP, and returns
//! the successor EIP. Family declaration adapters complete ordinary bodies with fallthrough.

use wasm86_compiler::{BuildError, Val, I32};

use super::{Location, Operand, OperandSize};
use crate::execution::ExecutionBuilder;
use crate::flags::Condition;

pub(super) type NullaryHandler = for<'body, 'module> fn(
    execution: &mut ExecutionBuilder<'body, 'module>,
    condition: Option<Condition>,
    fallthrough_eip: Val<I32>,
) -> Result<Val<I32>, BuildError>;

pub(super) type BinaryHandler = for<'body, 'module> fn(
    execution: &mut ExecutionBuilder<'body, 'module>,
    left: Operand<Val<I32>>,
    right: Operand<Val<I32>>,
    condition: Option<Condition>,
    fallthrough_eip: Val<I32>,
) -> Result<Val<I32>, BuildError>;

pub(super) type UnaryHandler = for<'body, 'module> fn(
    execution: &mut ExecutionBuilder<'body, 'module>,
    operand: Operand<Val<I32>>,
    condition: Option<Condition>,
    fallthrough_eip: Val<I32>,
) -> Result<Val<I32>, BuildError>;

pub(super) type TernaryHandler = for<'body, 'module> fn(
    execution: &mut ExecutionBuilder<'body, 'module>,
    destination: Location<Val<I32>>,
    first_source: Operand<Val<I32>>,
    second_source: Operand<Val<I32>>,
    condition: Option<Condition>,
    fallthrough_eip: Val<I32>,
) -> Result<Val<I32>, BuildError>;

/// Declaration adapters supply these functions before their field bindings are paired.
#[derive(Clone, Copy)]
pub(super) enum Handler {
    Nullary(NullaryHandler),
    Binary(BinaryHandler),
    Unary(UnaryHandler),
    Ternary(TernaryHandler),
}

#[derive(Clone, Copy)]
pub(super) struct SizedHandlers<H> {
    pub(super) word: H,
    pub(super) dword: H,
}

impl<H: Copy> SizedHandlers<H> {
    pub(super) const fn fixed(handler: H) -> Self {
        Self {
            word: handler,
            dword: handler,
        }
    }

    pub(super) fn resolve(self, size: OperandSize) -> H {
        match size {
            OperandSize::Word => self.word,
            OperandSize::Dword => self.dword,
        }
    }
}

/// A handler and its arguments share one shape, from field bindings to decoded operands.
#[derive(Clone, Copy)]
pub(super) enum HandlerCall<L, O> {
    Nullary {
        handler: NullaryHandler,
    },
    Binary {
        handler: BinaryHandler,
        left: O,
        right: O,
    },
    Unary {
        handler: UnaryHandler,
        operand: O,
    },
    Ternary {
        handler: TernaryHandler,
        destination: L,
        first_source: O,
        second_source: O,
    },
}
