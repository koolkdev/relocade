mod binding;
pub(super) mod declarations;
mod encoding;
mod modrm;
mod opcodes;

#[cfg(test)]
mod tests;

pub(super) use declarations::instruction_families;
pub(crate) use encoding::{DecodedFields, Encoding, FieldWidth, ImmediateWidth, OperandEncoding};
pub(crate) use modrm::ModRmSelector;
pub(crate) use opcodes::forms_by_opcode;

use super::{
    handlers::{HandlerCall, SizedHandlers},
    Group1Prefix, InstructionFlow, OperandSize, PrefixState, Requirements,
};
use crate::flags::Condition;
use crate::register::NamedRegister;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum OpcodeMap {
    #[default]
    Primary,
    Extended,
}

/// Instruction effects used by decoding and lowering. Descriptor effects carry
/// the first opcode byte reported by an unsupported-instruction exit.
#[derive(Clone, Copy)]
pub(super) enum ExecutionEffect {
    ControlTransfer,
    DescriptorLoad(u8),
    DescriptorQuery(u8),
    /// Device callbacks may change instruction routing; resume through dispatch.
    PortIo,
    /// Complete prior work and resume through dispatch before the next fetch.
    Serializing,
    /// Execution always raises a guest fault, so no successor belongs to the block.
    UnconditionalFault,
}

/// Statically known continuations of instructions that end an execution block.
#[derive(Clone, Copy)]
pub(crate) enum ControlFlow {
    Fallthrough,
    Jump,
    Branch,
    Call,
    FarCall,
}

impl OpcodeMap {
    pub(crate) const fn bytes(self) -> u32 {
        match self {
            Self::Primary => 1,
            Self::Extended => 2,
        }
    }
}

/// The register file selected by a form, independently of its encoded field.
#[derive(Clone, Copy)]
pub(super) enum RegisterClass {
    Gpr,
    Mmx,
    Xmm,
}

/// A location's role is independent of the fields needed to decode it.
#[derive(Clone, Copy)]
pub(super) enum LocationBinding {
    Register(RegisterClass),
    Rm(RegisterClass),
    /// A named register view, independent of any encoded register field.
    FixedRegister(NamedRegister),
    AbsoluteOffset,
}

#[derive(Clone, Copy)]
pub(super) enum OperandBinding {
    Location(LocationBinding),
    X87StackIndex,
    Segment(crate::Segment),
    Immediate(usize),
    /// An implicit literal, interpreted at the handler's logical operand width.
    Constant(u32),
    /// The r/m address fields form a value; register addressing is not accepted.
    RmAddress,
}

type HandlerBinding = HandlerCall<LocationBinding, OperandBinding>;

#[derive(Clone, Copy)]
pub(super) enum PrefixRule {
    /// Prefixes retain their operand-size, repetition and locking roles.
    Modifiers(Option<Group1Prefix>),
    /// Prefix presence selects an opcode variant.
    OpcodeSelector(MandatoryPrefix),
}

/// A prefix used as part of the opcode to select an instruction variant.
/// Its usual operand-size or repetition role does not apply to that form.
#[derive(Clone, Copy)]
pub(super) enum MandatoryPrefix {
    /// The unprefixed variant; segment and address-size prefixes can still apply.
    None,
    P66,
    F2,
    F3,
}

impl MandatoryPrefix {
    fn matches(self, prefixes: &PrefixState) -> bool {
        match self {
            Self::None => {
                !prefixes.selection().has_operand_override()
                    && prefixes.selection().group1().is_none()
            }
            Self::P66 => {
                prefixes.selection().has_operand_override()
                    && prefixes.selection().group1().is_none()
            }
            Self::F2 => {
                !prefixes.selection().has_operand_override()
                    && prefixes.selection().group1() == Some(Group1Prefix::F2)
            }
            Self::F3 => {
                !prefixes.selection().has_operand_override()
                    && prefixes.selection().group1() == Some(Group1Prefix::F3)
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Form {
    pub(super) opcode: u8,
    pub(super) mask: u8,
    pub(crate) map: OpcodeMap,
    prefix: PrefixRule,
    lockable: bool,
    pub(crate) encoding: Encoding,
    pub(crate) modrm: Option<ModRmSelector>,
    handlers: SizedHandlers<HandlerBinding>,
    pub(super) condition: Option<Condition>,
    pub(super) effects: &'static [ExecutionEffect],
    /// Block boundaries can still have discoverable continuations.
    pub(super) control_flow: Option<ControlFlow>,
    pub(super) requirements: Requirements,
}

impl Form {
    fn ends_block(&self) -> bool {
        self.control_flow.is_some()
            || self
                .effects
                .iter()
                .any(|effect| !matches!(effect, ExecutionEffect::DescriptorQuery(_)))
    }

    /// Prefix bytes select a form independently of operand and address widths.
    pub(crate) fn accepts_prefixes(&self, prefixes: &PrefixState) -> bool {
        match self.prefix {
            PrefixRule::Modifiers(required) => match prefixes.selection().group1() {
                Some(Group1Prefix::F0) => self.lockable,
                prefix => required == prefix,
            },
            PrefixRule::OpcodeSelector(required) => required.matches(prefixes),
        }
    }

    /// Fixed-width forms share their decoder body across both code defaults.
    pub(crate) fn uses_operand_size(&self) -> bool {
        matches!(self.handlers, SizedHandlers::OperandSize { .. })
            || self
                .encoding
                .immediates
                .iter()
                .any(|width| matches!(width, Some(ImmediateWidth::OperandSize)))
    }

    /// Select the handler and immediate widths after the decoder resolves operand size.
    pub(crate) fn resolve(
        &self,
        prefixes: &PrefixState,
        operand_size: super::OperandSize,
    ) -> Option<ResolvedForm> {
        if !self.accepts_prefixes(prefixes) {
            return None;
        }
        Some(ResolvedForm {
            form: *self,
            operand_size,
            call: self.handlers.resolve(operand_size),
            locked: prefixes.selection().group1() == Some(Group1Prefix::F0),
        })
    }

    /// The caller has already selected this form's opcode map.
    pub(crate) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }

    pub(crate) fn matches_modrm(&self, modrm: u8, prefixes: &PrefixState) -> bool {
        self.modrm.is_some_and(|selector| selector.matches(modrm))
            && (modrm >> 6 != 3 || self.accepts_register_rm(prefixes))
    }

    pub(crate) fn accepts_register_rm(&self, prefixes: &PrefixState) -> bool {
        self.modrm.is_some_and(ModRmSelector::accepts_register)
            && prefixes.selection().group1() != Some(Group1Prefix::F0)
    }

    pub(crate) fn accepts_memory_rm(&self) -> bool {
        self.modrm.is_some_and(ModRmSelector::accepts_memory)
    }
}

/// Operand-sized immediate widths and the concrete instruction handler.
#[derive(Clone)]
pub(crate) struct ResolvedForm {
    form: Form,
    operand_size: OperandSize,
    call: HandlerBinding,
    locked: bool,
}

/// The destination operand encoded in a direct CALL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallTarget {
    /// CS-relative offset after applying the displacement and operand size.
    Near(u32),
    /// The selector and offset from the far pointer in the instruction.
    /// Protected-mode execution must resolve the selector to determine its destination.
    Far { selector: u16, offset: u32 },
}

impl ResolvedForm {
    pub(crate) fn call_target(
        &self,
        immediates: [Option<u32>; 2],
        fallthrough: u32,
    ) -> Option<CallTarget> {
        let immediate = immediates[0]?;
        let mask = self.offset_mask();
        match self.form.control_flow {
            Some(ControlFlow::Call) => {
                Some(CallTarget::Near(fallthrough.wrapping_add(immediate) & mask))
            }
            Some(ControlFlow::FarCall) => Some(CallTarget::Far {
                selector: immediates[1]? as u16,
                offset: immediate & mask,
            }),
            _ => None,
        }
    }

    fn offset_mask(&self) -> u32 {
        match self.operand_size {
            OperandSize::Word => 0xffff,
            OperandSize::Dword => u32::MAX,
        }
    }

    pub(crate) fn flow(
        &self,
        immediates: [Option<u32>; 2],
        next_eip: u32,
        target: crate::ExecutionTarget,
    ) -> InstructionFlow {
        if self.form.requirements.known_fault(target).is_some() {
            return InstructionFlow::Stop;
        }
        let relative_target = || {
            let displacement = immediates[0].expect("relative transfers have a displacement");
            next_eip.wrapping_add(displacement) & self.offset_mask()
        };
        match self.form.control_flow {
            Some(ControlFlow::Fallthrough) => InstructionFlow::Fallthrough,
            Some(ControlFlow::Jump) => InstructionFlow::Jump {
                target: relative_target(),
            },
            Some(ControlFlow::Branch) => InstructionFlow::Branch {
                taken: relative_target(),
            },
            Some(ControlFlow::Call) => InstructionFlow::NearCall {
                return_eip: next_eip & self.offset_mask(),
            },
            Some(ControlFlow::FarCall) => InstructionFlow::FarCall {
                return_eip: next_eip & self.offset_mask(),
            },
            None if self.form.ends_block() => InstructionFlow::Stop,
            None => InstructionFlow::Continue,
        }
    }

    pub(crate) fn encoding(&self) -> Encoding {
        self.form.encoding
    }

    pub(crate) fn immediate_width(&self, field: ImmediateWidth) -> FieldWidth {
        field.width(self.operand_size)
    }
}
