//! Adapts typed instruction bodies to concrete handlers for each operand width.
//!
//! Both operand shapes receive the bound condition and fallthrough EIP, and return
//! the successor EIP. The adapters below complete ordinary bodies with fallthrough.

use wasm86_compiler::{BuildError, Val, I32};

use super::{Location, Operand, OperandSize};
use crate::{execution::ExecutionBuilder, flags::Condition};

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

/// These are Rust code-generation functions, selected while decoding a form.
#[derive(Clone, Copy)]
pub(super) enum Handler {
    Binary(BinaryHandler),
    Unary(UnaryHandler),
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

pub(super) struct IntegerHandlers<H> {
    pub(super) byte: H,
    pub(super) sized: SizedHandlers<H>,
}

/// A concrete handler together with its decoded operand arguments.
pub(super) enum HandlerCall<V> {
    Binary {
        handler: BinaryHandler,
        left: Location<V>,
        right: Operand<V>,
    },
    Unary {
        handler: UnaryHandler,
        operand: Operand<V>,
    },
}

// Widths and constant arguments select concrete Rust functions; handlers remain ordinary code.
macro_rules! binary_handlers {
    ($handler:ident, source = $source:ty $(, $argument:expr)*) => {
        SizedHandlers {
            word: binary_handlers!(@pair $handler, I16, $source $(, $argument)*),
            dword: binary_handlers!(@pair $handler, I32, $source $(, $argument)*),
        }
    };
    ($handler:ident $(, $argument:expr)*) => {
        IntegerHandlers {
            byte: binary_handlers!(@pair $handler, I8, I8 $(, $argument)*),
            sized: SizedHandlers {
                word: binary_handlers!(@pair $handler, I16, I16 $(, $argument)*),
                dword: binary_handlers!(@pair $handler, I32, I32 $(, $argument)*),
            },
        }
    };
    (@pair $handler:ident, $destination:ty, $source:ty $(, $argument:expr)*) => {
        Handler::Binary(|execution, destination, source, _, fallthrough| {
            $handler(execution, TypedLocation::<$destination>::new(destination), Input::<$source>::new(source) $(, $argument)*)?;
            Ok(fallthrough)
        })
    };
}

macro_rules! unary_handlers {
    ($handler:ident, $operand:ident, sized $(, $argument:expr)*) => {
        SizedHandlers {
            word: unary_handlers!(@width $handler, $operand, I16 $(, $argument)*),
            dword: unary_handlers!(@width $handler, $operand, I32 $(, $argument)*),
        }
    };
    ($handler:ident, $operand:ident $(, $argument:expr)*) => {
        IntegerHandlers {
            byte: unary_handlers!(@width $handler, $operand, I8 $(, $argument)*),
            sized: unary_handlers!($handler, $operand, sized $(, $argument)*),
        }
    };
    (@width $handler:ident, $operand:ident, $width:ty $(, $argument:expr)*) => {
        Handler::Unary(|execution, operand, _, fallthrough| {
            $handler(execution, unary_handlers!(@operand $operand, $width, operand) $(, $argument)*)?;
            Ok(fallthrough)
        })
    };
    (@operand Input, $width:ty, $operand:ident) => { Input::<$width>::new($operand) };
    (@operand TypedLocation, $width:ty, $operand:ident) => { TypedLocation::<$width>::from_operand($operand) };
}

pub(super) use {binary_handlers, unary_handlers};
