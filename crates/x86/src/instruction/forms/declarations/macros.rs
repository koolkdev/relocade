//! Family tables expose opcode bytes, operand order and widths at the use site.

macro_rules! instruction_families {
    ($($family:ident {
        execute: $handler:ident $(::<$size:tt>)? $(($($argument:expr),* $(,)?))?;
        $(effects: [$($effect:ident),* $(,)?];)?
        forms $rows:tt
    })+) => {
        pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
            use crate::instruction::forms::declarations::*;
            const FAMILIES: &[&[&[Form]]] = &[$(
                declaration_family!([$handler $(::<$size>)?; $($($argument),*)?] [$($($effect),*)?] $rows)
            ),+];
            FAMILIES.iter().flat_map(|rows| rows.iter().flat_map(|forms| forms.iter()))
        }
    };
}

macro_rules! declaration_family {
    ($call:tt $effects:tt {
        $($($prefix:ident)? $opcode:literal $($extended:literal)? $(+ $pattern:ident)? $(/ $extension:literal)? =>
            $width:ident $operands:tt $(| $other_width:ident $other_operands:tt)? $($lockable:ident)?;
        )+
    }) => {
        &[$({
            const FORM: Form = {
                $(same_layout(declaration_operands!($operands), declaration_operands!($other_operands));)?
                Declaration {
                    opcode: Opcode {
                        map: declaration_opcode!(@map $opcode $($extended)?),
                        byte: declaration_opcode!(@byte $opcode $($extended)?),
                        group1_prefix: declaration_opcode!(@prefix $($prefix)?),
                        register_range: declaration_opcode!(@register $($pattern)?),
                        extension: declaration_opcode!(@extension $($extension)?),
                    },
                    operands: declaration_operands!($operands),
                    handlers: declaration_handlers!(
                        $effects [$($pattern)?] $call $width $operands $(| $other_width $other_operands)?
                    ),
                    effects: declaration_effects!($effects),
                    lockable: declaration_opcode!(@lockable $($lockable)?),
                }.form()
            };
            declaration_opcode!(@rows FORM; $($pattern)?)
        }),+]
    };
}

macro_rules! declaration_operands {
    (($($operand:ident $(($value:expr))?),*)) => {
        &[$(operand_spec!($operand $(($value))?)),*]
    };
}

macro_rules! declaration_effects {
    ([$($effect:ident),*]) => { &[$(declaration_effect!($effect)),*] };
}

macro_rules! declaration_opcode {
    (@lockable lockable) => {
        true
    };
    (@lockable) => {
        false
    };
    (@prefix $prefix:ident) => {
        Some(crate::instruction::Group1Prefix::$prefix)
    };
    (@prefix) => {
        None
    };
    (@map $opcode:literal) => {
        OpcodeMap::Primary
    };
    (@map $escape:literal $opcode:literal) => {{
        assert!($escape == 0x0f, "the extended opcode map starts with 0F");
        OpcodeMap::Extended
    }};
    (@byte $opcode:literal) => {
        $opcode
    };
    (@byte $escape:literal $opcode:literal) => {
        $opcode
    };
    (@register reg) => {
        true
    };
    (@register cc) => {
        false
    };
    (@register) => {
        false
    };
    (@extension $extension:literal) => {
        Some($extension)
    };
    (@extension) => {
        None
    };
    (@rows $form:ident; cc) => {
        &condition_forms($form)
    };
    (@rows $form:ident; reg) => {
        &[$form]
    };
    (@rows $form:ident;) => {
        &[$form]
    };
}

macro_rules! declaration_effect {
    (segment_load) => {
        Effect::SegmentLoad
    };
    (memory_read) => {
        Effect::MemoryRead
    };
    (memory_write) => {
        Effect::MemoryWrite
    };
    (control_transfer) => {
        Effect::ControlTransfer
    };
    (unconditional_fault) => {
        Effect::UnconditionalFault
    };
}

macro_rules! operand_spec {
    (mem) => {
        OperandSpec::Memory
    };
    (segment($segment:expr)) => {
        OperandSpec::Segment($segment)
    };
    (rm) => {
        OperandSpec::Rm
    };
    (rm8) => {
        OperandSpec::Rm
    };
    (rm16) => {
        OperandSpec::Rm
    };
    (modrm_reg) => {
        OperandSpec::ModRmRegister
    };
    (opcode_reg) => {
        OperandSpec::OpcodeRegister
    };
    (accumulator) => {
        OperandSpec::FixedRegister(crate::register::NamedRegister::low(
            crate::register::Gpr32::Eax,
        ))
    };
    (AL) => {
        operand_spec!(accumulator)
    };
    (AH) => {
        OperandSpec::FixedRegister(crate::register::NamedRegister::AH)
    };
    (AX) => {
        operand_spec!(accumulator)
    };
    (EAX) => {
        operand_spec!(accumulator)
    };
    (CL) => {
        OperandSpec::FixedRegister(crate::register::NamedRegister::low(
            crate::register::Gpr32::Ecx,
        ))
    };
    (DX) => {
        OperandSpec::FixedRegister(crate::register::NamedRegister::low(
            crate::register::Gpr32::Edx,
        ))
    };
    (EDX) => {
        OperandSpec::FixedRegister(crate::register::NamedRegister::low(
            crate::register::Gpr32::Edx,
        ))
    };
    (moffs) => {
        OperandSpec::Offset
    };
    (address) => {
        OperandSpec::Address
    };
    (imm8) => {
        OperandSpec::Immediate(ImmediateWidth::Byte)
    };
    (imm16) => {
        OperandSpec::Immediate(ImmediateWidth::Word)
    };
    (imm) => {
        OperandSpec::Immediate(ImmediateWidth::OperandSize)
    };
    (signed_imm8) => {
        OperandSpec::Immediate(ImmediateWidth::SignedByte)
    };
    (rel8) => {
        OperandSpec::Immediate(ImmediateWidth::SignedByte)
    };
    (rel) => {
        OperandSpec::Immediate(ImmediateWidth::OperandSize)
    };
    (constant($value:expr)) => {
        OperandSpec::Constant($value)
    };
}

pub(in crate::instruction) use {
    declaration_effect, declaration_effects, declaration_family, declaration_opcode,
    declaration_operands, instruction_families, operand_spec,
};
