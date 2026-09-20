//! Adapts a row's operands to the ordinary Rust semantic body's argument types.

macro_rules! declaration_handlers {
    ($effects:tt $pattern:tt $call:tt no_operands()) => {
        SizedHandlers::fixed(declaration_adapter!($effects $pattern $call [];))
    };
    ($effects:tt $pattern:tt $call:tt byte($($operands:tt)*)) => {
        SizedHandlers::fixed(declaration_adapter!($effects $pattern $call [I8]; $($operands)*))
    };
    ($effects:tt $pattern:tt $call:tt word($($operands:tt)*)) => {
        SizedHandlers::fixed(declaration_adapter!($effects $pattern $call [I16]; $($operands)*))
    };
    ($effects:tt $pattern:tt $call:tt word_or_dword($($operands:tt)*)) => {
        SizedHandlers {
            word: declaration_adapter!($effects $pattern $call [I16]; $($operands)*),
            dword: declaration_adapter!($effects $pattern $call [I32]; $($operands)*),
        }
    };
    ($effects:tt $pattern:tt $call:tt word($($word:tt)*) | dword($($dword:tt)*)) => {
        SizedHandlers {
            word: declaration_adapter!($effects $pattern $call [I16]; $($word)*),
            dword: declaration_adapter!($effects $pattern $call [I32]; $($dword)*),
        }
    };
}

// All bodies receive typed operands. Transfers return the successor EIP;
// ordinary bodies complete with fallthrough. Effects can appear in any order.
macro_rules! declaration_adapter {
    ([control_transfer $(, $effect:ident)*] $($row:tt)*) => {
        declaration_adapter!(@result [control_transfer] $($row)*)
    };
    ([$effect:ident $(, $rest:ident)*] $pattern:tt $call:tt $width:tt; $($operands:tt)*) => {
        declaration_adapter!([$($rest),*] $pattern $call $width; $($operands)*)
    };
    ([] $($row:tt)*) => {
        declaration_adapter!(@result [] $($row)*)
    };
    (@result $result:tt $pattern:tt $call:tt [$($width:ty)?];) => {
        Handler::Nullary(|execution, _condition, fallthrough| {
            declaration_result!($result $pattern $call [$($width)?] execution, _condition, fallthrough;)
        })
    };
    (@result $result:tt $pattern:tt $call:tt [$width:ty]; $operand:ident $(($value:expr))?) => {
        Handler::Unary(|execution, operand, _condition, fallthrough| {
            declaration_result!($result $pattern $call [$width] execution, _condition, fallthrough;
                operand_value!($width, operand, $operand $(($value))?))
        })
    };
    (@result $result:tt $pattern:tt $call:tt [$width:ty]; $left:ident $(($left_value:expr))?, $right:ident $(($right_value:expr))?) => {
        Handler::Binary(|execution, left, right, _condition, fallthrough| {
            declaration_result!($result $pattern $call [$width] execution, _condition, fallthrough;
                operand_value!($width, left, $left $(($left_value))?),
                operand_value!($width, right, $right $(($right_value))?))
        })
    };
    (@result $result:tt $pattern:tt $call:tt [$width:ty];
        $destination:ident $(($destination_value:expr))?,
        $first:ident $(($first_value:expr))?,
        $second:ident $(($second_value:expr))?
    ) => {
        Handler::Ternary(|execution, destination, first, second, _condition, fallthrough| {
            let destination = destination.into();
            declaration_result!($result $pattern $call [$width] execution, _condition, fallthrough;
                operand_value!($width, destination, $destination $(($destination_value))?),
                operand_value!($width, first, $first $(($first_value))?),
                operand_value!($width, second, $second $(($second_value))?))
        })
    };
}

macro_rules! declaration_result {
    ([control_transfer] $pattern:tt $call:tt $width:tt $execution:ident, $condition:ident, $fallthrough:ident; $($operand:expr),*) => {
        declaration_invoke!($call $width; $execution $(, $operand)*, $condition, $fallthrough)
    };
    ([] $pattern:tt $call:tt $width:tt $execution:ident, $condition:ident, $fallthrough:ident; $($operand:expr),*) => {{
        declaration_call!($pattern $call $width $execution, $condition; $($operand),*)?;
        Ok($fallthrough)
    }};
}

macro_rules! declaration_call {
    ([cc] $call:tt $width:tt $execution:ident, $condition:ident; $($operand:expr),*) => {
        declaration_invoke!($call $width; $execution $(, $operand)*, $condition.expect("+cc binds a condition"))
    };
    ([$($pattern:ident)?] $call:tt $width:tt $execution:ident, $condition:ident; $($operand:expr),*) => {
        declaration_invoke!($call $width; $execution $(, $operand)*)
    };
}

// An explicit ::<_> forwards the row's logical width independently of operand
// kind and arity. Ordinary calls infer types from their typed operands.
macro_rules! declaration_invoke {
    ([$handler:ident ::<_>; $($argument:expr),*] [$width:ty]; $($operand:expr),*) => {
        $handler::<$width>($($operand),* $(, $argument)*)
    };
    ([$handler:ident; $($argument:expr),*] $width:tt; $($operand:expr),*) => {
        $handler($($operand),* $(, $argument)*)
    };
}

macro_rules! operand_value {
    ($width:ty, $operand:ident, mem) => {{
        let crate::instruction::Operand::Location(crate::instruction::Location::Memory(address)) = $operand else {
            unreachable!("the form binds a memory addressing mode")
        };
        *address
    }};
    ($width:ty, $operand:ident, segment($value:expr)) => {{
        let crate::instruction::Operand::Segment(segment) = $operand else {
            unreachable!("the form binds a segment register")
        };
        segment
    }};
    ($width:ty, $operand:ident, constant($value:expr)) => { Input::new($operand) };
    ($width:ty, $operand:ident, imm8) => { Input::<I8>::new($operand) };
    ($width:ty, $operand:ident, imm16) => { Input::<I16>::new($operand) };
    ($width:ty, $operand:ident, imm) => { Input::<$width>::new($operand) };
    ($width:ty, $operand:ident, signed_imm8) => { Input::<$width>::new($operand) };
    ($width:ty, $operand:ident, rel8) => { Input::<I32>::new($operand) };
    ($width:ty, $operand:ident, rel) => { Input::<I32>::new($operand) };
    ($width:ty, $operand:ident, address) => { Input::<$width>::new($operand) };
    ($width:ty, $operand:ident, rm8) => { operand_value!(@location I8, $operand) };
    ($width:ty, $operand:ident, rm16) => { operand_value!(@location I16, $operand) };
    ($width:ty, $operand:ident, AL) => { operand_value!(@location I8, $operand) };
    ($width:ty, $operand:ident, AH) => { operand_value!(@location I8, $operand) };
    ($width:ty, $operand:ident, CL) => { operand_value!(@location I8, $operand) };
    ($width:ty, $operand:ident, AX) => { operand_value!(@location I16, $operand) };
    ($width:ty, $operand:ident, DX) => { operand_value!(@location I16, $operand) };
    ($width:ty, $operand:ident, EAX) => { operand_value!(@location I32, $operand) };
    ($width:ty, $operand:ident, EDX) => { operand_value!(@location I32, $operand) };
    ($width:ty, $operand:ident, rm) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, modrm_reg) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, opcode_reg) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, accumulator) => { operand_value!(@location $width, $operand) };
    ($width:ty, $operand:ident, moffs) => { operand_value!(@location $width, $operand) };
    (@location $width:ty, $operand:ident) => { TypedLocation::<$width>::from_operand($operand).into() };
}

pub(in crate::instruction) use {
    declaration_adapter, declaration_call, declaration_handlers, declaration_invoke,
    declaration_result, operand_value,
};
