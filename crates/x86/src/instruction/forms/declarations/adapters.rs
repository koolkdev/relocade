//! Adapts a row's operands to the ordinary Rust semantic body's argument types.

macro_rules! declaration_handlers {
    ($effects:tt $pattern:tt $call:tt byte($($operands:tt)*)) => {
        SizedHandlers::fixed(declaration_adapter!($effects $pattern $call I8; $($operands)*))
    };
    ($effects:tt $pattern:tt $call:tt word_or_dword($($operands:tt)*)) => {
        SizedHandlers {
            word: declaration_adapter!($effects $pattern $call I16; $($operands)*),
            dword: declaration_adapter!($effects $pattern $call I32; $($operands)*),
        }
    };
    ($effects:tt $pattern:tt $call:tt word($($word:tt)*) | dword($($dword:tt)*)) => {
        SizedHandlers {
            word: declaration_adapter!($effects $pattern $call I16; $($word)*),
            dword: declaration_adapter!($effects $pattern $call I32; $($dword)*),
        }
    };
}

// A transfer body already has the successor-returning ABI. Other bodies receive
// typed arguments and complete with fallthrough. Effects can appear in any order.
macro_rules! declaration_adapter {
    ([control_transfer $(, $effect:ident)*] $pattern:tt [$handler:ident] $width:ty; $operand:ident $(($value:expr))?) => {
        Handler::Unary($handler::<$width>)
    };
    ([control_transfer $(, $effect:ident)*] $($unsupported:tt)*) => {
        compile_error!("control-transfer bodies use one operand and return the successor EIP")
    };
    ([$effect:ident $(, $rest:ident)*] $pattern:tt $call:tt $width:ty; $($operands:tt)*) => {
        declaration_adapter!([$($rest),*] $pattern $call $width; $($operands)*)
    };
    ([] $pattern:tt $call:tt $width:ty; $operand:ident $(($value:expr))?) => {
        Handler::Unary(|execution, operand, _condition, fallthrough| {
            declaration_call!($pattern $call execution, _condition;
                operand_value!($width, operand, $operand $(($value))?))?;
            Ok(fallthrough)
        })
    };
    ([] $pattern:tt $call:tt $width:ty; $left:ident $(($left_value:expr))?, $right:ident $(($right_value:expr))?) => {
        Handler::Binary(|execution, left, right, _condition, fallthrough| {
            let left = left.into();
            declaration_call!($pattern $call execution, _condition;
                operand_value!($width, left, $left $(($left_value))?),
                operand_value!($width, right, $right $(($right_value))?))?;
            Ok(fallthrough)
        })
    };
    ([] $pattern:tt $call:tt $width:ty;
        $destination:ident $(($destination_value:expr))?,
        $first:ident $(($first_value:expr))?,
        $second:ident $(($second_value:expr))?
    ) => {
        Handler::Ternary(|execution, destination, first, second, _condition, fallthrough| {
            let destination = destination.into();
            declaration_call!($pattern $call execution, _condition;
                operand_value!($width, destination, $destination $(($destination_value))?),
                operand_value!($width, first, $first $(($first_value))?),
                operand_value!($width, second, $second $(($second_value))?))?;
            Ok(fallthrough)
        })
    };
}

macro_rules! declaration_call {
    ([cc] [$handler:ident $($argument:expr),*] $execution:ident, $condition:ident; $($operand:expr),+) => {
        $handler($execution, $($operand),+, $condition.expect("+cc binds a condition") $(, $argument)*)
    };
    ([$($pattern:ident)?] [$handler:ident $($argument:expr),*] $execution:ident, $condition:ident; $($operand:expr),+) => {
        $handler($execution, $($operand),+ $(, $argument)*)
    };
}

macro_rules! operand_value {
    ($width:ty, $operand:ident, constant($value:expr)) => { Input::new($operand) };
    ($width:ty, $operand:ident, imm8) => { Input::<I8>::new($operand) };
    ($width:ty, $operand:ident, imm16) => { Input::<I16>::new($operand) };
    ($width:ty, $operand:ident, imm) => { Input::<$width>::new($operand) };
    ($width:ty, $operand:ident, signed_imm8) => { Input::<$width>::new($operand) };
    ($width:ty, $operand:ident, address) => { Input::<$width>::new($operand) };
    ($width:ty, $operand:ident, rm8) => { operand_value!(@location I8, $operand) };
    ($width:ty, $operand:ident, rm16) => { operand_value!(@location I16, $operand) };
    ($width:ty, $operand:ident, AL) => { operand_value!(@location I8, $operand) };
    ($width:ty, $operand:ident, CL) => { operand_value!(@location I8, $operand) };
    ($width:ty, $operand:ident, AX) => { operand_value!(@location I16, $operand) };
    ($width:ty, $operand:ident, DX) => { operand_value!(@location I16, $operand) };
    ($width:ty, $operand:ident, EAX) => { operand_value!(@location I32, $operand) };
    ($width:ty, $operand:ident, EDX) => { operand_value!(@location I32, $operand) };
    ($width:ty, $operand:ident, rm) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, modrm_reg) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, opcode_reg) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, accumulator) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, moffs32) => { operand_value!(@location $width, $operand) };
    (@location $width:ty, $operand:ident) => { TypedLocation::<$width>::from_operand($operand).into() };
}

pub(in crate::instruction) use {
    declaration_adapter, declaration_call, declaration_handlers, operand_value,
};
