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
    left: Location<Val<I32>>,
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

/// These are Rust code-generation functions, selected while decoding a form.
#[derive(Clone, Copy)]
pub(super) enum Handler {
    Nullary(NullaryHandler),
    Binary(BinaryHandler),
    Unary(UnaryHandler),
    Ternary(TernaryHandler),
}

#[derive(Clone, Copy)]
pub(super) struct SizedHandlers {
    pub(super) word: Handler,
    pub(super) dword: Handler,
}

impl SizedHandlers {
    pub(super) const fn fixed(handler: Handler) -> Self {
        Self {
            word: handler,
            dword: handler,
        }
    }

    pub(super) fn resolve(self, size: OperandSize) -> Handler {
        match size {
            OperandSize::Word => self.word,
            OperandSize::Dword => self.dword,
        }
    }
}

/// A concrete handler together with its decoded operand arguments.
pub(super) enum HandlerCall<V> {
    Nullary {
        handler: NullaryHandler,
    },
    Binary {
        handler: BinaryHandler,
        left: Location<V>,
        right: Operand<V>,
    },
    Unary {
        handler: UnaryHandler,
        operand: Operand<V>,
    },
    Ternary {
        handler: TernaryHandler,
        destination: Location<V>,
        first_source: Operand<V>,
        second_source: Operand<V>,
    },
}
