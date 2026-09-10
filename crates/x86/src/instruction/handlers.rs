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

// Widths and constant arguments select concrete Rust functions. Conditional bodies
// receive the condition bound by their form; all bodies remain ordinary Rust code.
macro_rules! typed_operand {
    (Input, $width:ty, $operand:ident) => {
        Input::<$width>::new($operand)
    };
    (TypedLocation, $width:ty, $operand:ident) => {
        TypedLocation::<$width>::from_operand($operand)
    };
}

macro_rules! binary_handlers {
    ($handler:ident, source = $source:ty $(, $argument:expr)*) => {
        SizedHandlers {
            word: binary_handlers!(@pair $handler, I16, Input, $source [] $(, $argument)*),
            dword: binary_handlers!(@pair $handler, I32, Input, $source [] $(, $argument)*),
        }
    };
    ($handler:ident, right = $right:ident $(, $argument:expr)*) => {
        binary_handlers!(@widths $handler, $right [] $(, $argument)*)
    };
    ($handler:ident, condition $(, $argument:expr)*) => {
        binary_handlers!(@widths $handler, Input [condition] $(, $argument)*)
    };
    ($handler:ident $(, $argument:expr)*) => {
        binary_handlers!(@widths $handler, Input [] $(, $argument)*)
    };
    (@widths $handler:ident, $right:ident [$($condition:ident)?] $(, $argument:expr)*) => {
        IntegerHandlers {
            byte: binary_handlers!(@pair $handler, I8, $right, I8 [$($condition)?] $(, $argument)*),
            sized: SizedHandlers {
                word: binary_handlers!(@pair $handler, I16, $right, I16 [$($condition)?] $(, $argument)*),
                dword: binary_handlers!(@pair $handler, I32, $right, I32 [$($condition)?] $(, $argument)*),
            },
        }
    };
    (@pair $handler:ident, $left_width:ty, $right:ident, $right_width:ty [$($condition:ident)?] $(, $argument:expr)*) => {
        Handler::Binary(|execution, left, right, _bound_condition, fallthrough| {
            $(let $condition = _bound_condition.expect("condition-dependent forms bind a condition");)?
            $handler(execution, TypedLocation::<$left_width>::new(left), typed_operand!($right, $right_width, right) $(, $condition)? $(, $argument)*)?;
            Ok(fallthrough)
        })
    };
}

macro_rules! unary_handlers {
    ($handler:ident, $operand:ident, width = $width:ty, condition $(, $argument:expr)*) => {
        unary_handlers!(@width $handler, $operand, $width [condition] $(, $argument)*)
    };
    ($handler:ident, $operand:ident, width = $width:ty $(, $argument:expr)*) => {
        unary_handlers!(@width $handler, $operand, $width [] $(, $argument)*)
    };
    ($handler:ident, $operand:ident, sized $(, $argument:expr)*) => {
        SizedHandlers {
            word: unary_handlers!(@width $handler, $operand, I16 [] $(, $argument)*),
            dword: unary_handlers!(@width $handler, $operand, I32 [] $(, $argument)*),
        }
    };
    ($handler:ident, $operand:ident $(, $argument:expr)*) => {
        IntegerHandlers {
            byte: unary_handlers!(@width $handler, $operand, I8 [] $(, $argument)*),
            sized: unary_handlers!($handler, $operand, sized $(, $argument)*),
        }
    };
    (@width $handler:ident, $operand:ident, $width:ty [$($condition:ident)?] $(, $argument:expr)*) => {
        Handler::Unary(|execution, operand, _bound_condition, fallthrough| {
            $(let $condition = _bound_condition.expect("condition-dependent forms bind a condition");)?
            $handler(execution, typed_operand!($operand, $width, operand) $(, $condition)? $(, $argument)*)?;
            Ok(fallthrough)
        })
    };
}

pub(super) use {binary_handlers, typed_operand, unary_handlers};
