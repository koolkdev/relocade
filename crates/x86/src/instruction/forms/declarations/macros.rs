//! Family tables expose opcode bytes, operand order and widths at the use site.
//! `register(xmm_rm)` derives register-only ModRM addressing and handler types.

macro_rules! instruction_families {
    ($($family:ident {
        execute: $handler:ident $(::<$size:tt>)? $(($($argument:expr),* $(,)?))?;
        $(effects: [$($effect:ident),* $(,)?];)?
        $(availability: $availability:ident;)?
        $(requires: [$($feature:ident),* $(,)?];)?
        forms $rows:tt
    })+) => {
        pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
            use crate::instruction::forms::declarations::*;
            // Runtime decoder tables identify forms across catalog traversals.
            static FAMILIES: &[&[&[Form]]] = &[$(
                declaration_family!([$handler $(::<$size>)?; $($($argument),*)?] [$($($effect),*)?] [$($availability)?] [$($($feature),*)?] $rows)
            ),+];
            FAMILIES.iter().flat_map(|rows| rows.iter().flat_map(|forms| forms.iter()))
        }
    };
}

macro_rules! declaration_family {
    ($call:tt $effects:tt $availability:tt $features:tt {
        $($($prefix:ident)? $opcode:literal $($extended:literal)? $(+ $pattern:ident)? $(/ $extension:literal)? $(@ $modrm:literal $(+ $modrm_range:ident)?)? =>
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
                        prefix: declaration_opcode!(@prefix $($prefix)?),
                        register_range: declaration_opcode!(@register $($pattern)?),
                        modrm: declaration_opcode!(@modrm [$($extension)?] [$($modrm $(+ $modrm_range)?)?]),
                    },
                    operands: declaration_operands!($operands),
                    handlers: declaration_handlers!(
                        $effects [$($pattern)?] $call $width $operands $(| $other_width $other_operands)?
                    ),
                    effects: declaration_effects!($effects),
                    availability: declaration_opcode!(@availability $availability),
                    features: declaration_opcode!(@features $features),
                    lockable: declaration_opcode!(@lockable $($lockable)?),
                }.form()
            };
            declaration_opcode!(@rows FORM; $($pattern)?)
        }),+]
    };
}

macro_rules! declaration_operands {
    (($($operand:ident $(($($value:tt)*))?),*)) => {
        &[$(operand_spec!($operand $(($($value)*))?)),*]
    };
}

macro_rules! declaration_effects {
    ([$($effect:ident),*]) => { &[$(declaration_effect!($effect)),*] };
}

macro_rules! declaration_opcode {
    (@features [$($feature:ident),*]) => {
        &[$(crate::CpuFeature::$feature),*]
    };
    (@availability []) => {
        crate::instruction::Availability::AllModes
    };
    (@availability [$mode:ident]) => {
        crate::instruction::Availability::$mode
    };
    (@lockable lockable) => {
        true
    };
    (@lockable) => {
        false
    };
    (@prefix NP) => {
        PrefixRule::OpcodeSelector(crate::instruction::forms::MandatoryPrefix::None)
    };
    (@prefix P66) => {
        PrefixRule::OpcodeSelector(crate::instruction::forms::MandatoryPrefix::P66)
    };
    (@prefix PF2) => {
        PrefixRule::OpcodeSelector(crate::instruction::forms::MandatoryPrefix::F2)
    };
    (@prefix PF3) => {
        PrefixRule::OpcodeSelector(crate::instruction::forms::MandatoryPrefix::F3)
    };
    (@prefix $prefix:ident) => {
        PrefixRule::Modifiers(Some(crate::instruction::Group1Prefix::$prefix))
    };
    (@prefix) => {
        PrefixRule::Modifiers(None)
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
    (@modrm [$extension:literal] []) => {
        Some(ModRmSelector::extension($extension))
    };
    (@modrm [] [$modrm:literal]) => {
        Some(ModRmSelector::byte($modrm))
    };
    (@modrm [] [$modrm:literal + rm]) => {
        Some(ModRmSelector::register_range($modrm))
    };
    (@modrm [] []) => {
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
    (interrupt_control) => {
        ExecutionEffect::InterruptControl
    };
    (serializing) => {
        ExecutionEffect::Serializing
    };
    (port_io) => {
        ExecutionEffect::PortIo
    };
    (segment_load) => {
        ExecutionEffect::SegmentLoad
    };
    (control_transfer) => {
        ExecutionEffect::ControlTransfer
    };
    (unconditional_fault) => {
        ExecutionEffect::UnconditionalFault
    };
}

macro_rules! operand_spec {
    (register($operand:ident)) => {
        operand_spec!($operand).register()
    };
    (mm) => {
        OperandSpec::ModRmRegister(crate::instruction::forms::RegisterClass::Mmx)
    };
    (mm_rm) => {
        OperandSpec::Rm(crate::instruction::forms::RegisterClass::Mmx)
    };
    (xmm) => {
        OperandSpec::ModRmRegister(crate::instruction::forms::RegisterClass::Xmm)
    };
    (xmm_rm) => {
        OperandSpec::Rm(crate::instruction::forms::RegisterClass::Xmm)
    };
    (st) => {
        OperandSpec::X87StackIndex
    };
    (mem16) => {
        OperandSpec::Memory
    };
    (mem) => {
        OperandSpec::Memory
    };
    (segment($segment:expr)) => {
        OperandSpec::Segment($segment)
    };
    (rm) => {
        OperandSpec::Rm(crate::instruction::forms::RegisterClass::Gpr)
    };
    (rm8) => {
        OperandSpec::Rm(crate::instruction::forms::RegisterClass::Gpr)
    };
    (rm16) => {
        OperandSpec::Rm(crate::instruction::forms::RegisterClass::Gpr)
    };
    (modrm_reg) => {
        OperandSpec::ModRmRegister(crate::instruction::forms::RegisterClass::Gpr)
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
