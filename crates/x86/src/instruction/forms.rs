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
    Availability, Group1Prefix, OperandSize, PrefixState, Requirements,
};
use crate::register::NamedRegister;
use crate::{address::AddressSize, flags::Condition};

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum OpcodeMap {
    #[default]
    Primary,
    Extended,
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
    pub(super) ends_block: bool,
    /// Block boundaries can still have discoverable continuations.
    pub(super) control_flow: Option<ControlFlow>,
    pub(super) requirements: Requirements,
}

impl Form {
    /// Resolve prefix meaning before either decoder reads operand fields.
    pub(crate) fn resolve(&self, prefixes: &PrefixState) -> Option<ResolvedForm> {
        let accepts_prefix = match self.prefix {
            PrefixRule::Modifiers(required) => match prefixes.selection().group1() {
                Some(Group1Prefix::F0) => self.lockable,
                prefix => required == prefix,
            },
            PrefixRule::OpcodeSelector(required) => required.matches(prefixes),
        };
        if !accepts_prefix {
            return None;
        }
        let operand_size = prefixes.selection().operand_size();
        Some(ResolvedForm {
            form: *self,
            operand_size,
            address_size: prefixes.selection().address_size(),
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

/// Physical fetch widths and the concrete handler are selected from the prefix state.
#[derive(Clone)]
pub(crate) struct ResolvedForm {
    form: Form,
    operand_size: OperandSize,
    address_size: AddressSize,
    call: HandlerBinding,
    locked: bool,
}

pub(crate) enum CallTarget {
    Near(u32),
    Far { selector: u16, offset: u32 },
}

impl ResolvedForm {
    pub(crate) fn call_target(
        &self,
        immediates: [Option<u32>; 2],
        fallthrough: u32,
    ) -> Option<CallTarget> {
        let immediate = immediates[0]?;
        let mask = match self.operand_size {
            OperandSize::Word => 0xffff,
            OperandSize::Dword => u32::MAX,
        };
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

    pub(crate) fn successors(
        &self,
        immediates: [Option<u32>; 2],
        fallthrough: u32,
        target: crate::ExecutionTarget,
    ) -> [Option<u32>; 2] {
        if self.form.requirements.fault(target).is_some() {
            return [None, None];
        }
        let mask = match self.operand_size {
            OperandSize::Word => 0xffff,
            OperandSize::Dword => u32::MAX,
        };
        match self.form.control_flow {
            Some(ControlFlow::Fallthrough) => [Some(fallthrough), None],
            Some(ControlFlow::Jump | ControlFlow::Branch) => {
                let destination = fallthrough
                    .wrapping_add(immediates[0].expect("relative transfers have a displacement"))
                    & mask;
                [
                    Some(destination),
                    matches!(self.form.control_flow, Some(ControlFlow::Branch))
                        .then_some(fallthrough),
                ]
            }
            // Protected far transfers need destination-context handling before
            // discovery follows their return continuations.
            Some(ControlFlow::FarCall) if target.profile != crate::ExecutionProfile::Real16 => {
                [None, None]
            }
            // CALL saves an operand-sized return offset. Real16 far returns also
            // restore the caller's CS, so its continuation uses the same reader.
            Some(ControlFlow::Call | ControlFlow::FarCall) => [Some(fallthrough & mask), None],
            None => [None, None],
        }
    }

    pub(crate) fn ends_block(&self, target: crate::ExecutionTarget) -> bool {
        self.form.ends_block || self.form.requirements.fault(target).is_some()
    }

    pub(crate) fn address_size(&self) -> AddressSize {
        self.address_size
    }

    pub(crate) fn address_width(&self) -> FieldWidth {
        match self.address_size {
            AddressSize::Bits16 => FieldWidth::Word,
            AddressSize::Bits32 => FieldWidth::Dword,
        }
    }

    pub(crate) fn encoding(&self) -> Encoding {
        self.form.encoding
    }

    pub(crate) fn immediate_width(&self, field: ImmediateWidth) -> FieldWidth {
        field.width(self.operand_size)
    }
}
