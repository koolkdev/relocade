mod address;
#[cfg(test)]
mod tests;

use super::DecodeState;

use crate::{
    address::AddressSize,
    instruction::{
        DecodedFields, DecodedInstruction, FieldWidth, ImmediateWidth, Location, OperandEncoding,
        Prefix, ResolvedForm, SegmentOverride, EXTENDED_OPCODE_ESCAPE, MAX_INSTRUCTION_BYTES,
    },
    register::{RegisterCode, RegisterSelector},
    segment::{Segment, SegmentDefaultSize},
    BlockError, CallTarget, CompilerConfig, InstructionFlow,
};

/// One instruction decoded from captured bytes using the supplied CS default size.
/// It owns the decoded operands and can move between threads.
pub struct SnapshotInstruction {
    form: ResolvedForm,
    address_size: AddressSize,
    fields: DecodedFields<u32, RegisterCode, SnapshotRm>,
    segment_override: Option<Segment>,
    eip: u32,
    next_eip: u32,
    code_default_size: SegmentDefaultSize,
}

impl SnapshotInstruction {
    /// Decodes the first instruction in `bytes`, beginning at the given CS-relative EIP.
    pub fn decode(bytes: &[u8], eip: u32, size: SegmentDefaultSize) -> Result<Self, BlockError> {
        snapshot(bytes, eip, size).map(|(instruction, _)| instruction)
    }

    /// CS-relative offset of the first encoding byte.
    pub fn eip(&self) -> u32 {
        self.eip
    }

    /// Offset immediately after the encoding, with 32-bit wrapping.
    pub fn next_eip(&self) -> u32 {
        self.next_eip
    }

    /// The captured CS default size before applying instruction prefixes.
    pub fn code_default_size(&self) -> SegmentDefaultSize {
        self.code_default_size
    }

    /// Classifies transfers and block boundaries, including faults known from the target.
    pub fn flow(&self, config: impl Into<CompilerConfig>) -> InstructionFlow {
        self.form
            .flow(self.fields.immediates, self.next_eip, config.into())
    }

    /// Encoded destination for a direct CALL. Indirect calls and other instructions return `None`.
    pub fn call_target(&self) -> Option<CallTarget> {
        self.form.call_target(self.fields.immediates, self.next_eip)
    }

    pub(crate) fn bind(self) -> DecodedInstruction<u32, u32> {
        self.form.bind(
            self.fields
                .map_locations(RegisterSelector::Fixed, SnapshotRm::bind),
            self.eip,
            self.next_eip,
            &self
                .segment_override
                .map_or(SegmentOverride::None, SegmentOverride::Fixed),
            self.address_size.mask(),
        )
    }
}

enum SnapshotRm {
    Register(RegisterCode),
    Memory(address::SnapshotAddress),
}

impl SnapshotRm {
    fn bind(self) -> Location<u32> {
        match self {
            Self::Register(code) => Location::Register(RegisterSelector::Fixed(code).into()),
            Self::Memory(address) => Location::Memory(address.bind().memory().into()),
        }
    }
}

pub(crate) fn snapshot(
    bytes: &[u8],
    instruction_eip: u32,
    default_size: SegmentDefaultSize,
) -> Result<(SnapshotInstruction, &[u8]), BlockError> {
    let mut cursor = SnapshotCursor {
        bytes,
        instruction_eip,
        offset: 0,
    };
    let mut state = DecodeState::default();
    let mut segment_override = None;
    let mut opcode = loop {
        let byte = cursor.byte()?;
        if let Some(prefix) = Prefix::from_byte(byte) {
            if let Prefix::Segment(segment) = prefix {
                segment_override = Some(segment);
            } else {
                state = state.with_prefix(prefix);
            }
        } else {
            break byte;
        }
    };
    if opcode == EXTENDED_OPCODE_ESCAPE {
        state = state.extended().ok_or(BlockError::UnsupportedInstruction {
            address: instruction_eip,
        })?;
        opcode = cursor.byte()?;
    }
    let mut candidates = state.forms().filter(|form| form.matches(opcode));
    let first = candidates
        .next()
        .ok_or(BlockError::UnsupportedInstruction {
            address: instruction_eip,
        })?;
    let (form, modrm) = if first.encoding.has_modrm() {
        let modrm = cursor.byte()?;
        let form = std::iter::once(first)
            .chain(candidates)
            .find(|form| form.matches_modrm(modrm, &state.prefixes))
            .ok_or(BlockError::UnsupportedInstruction {
                address: instruction_eip,
            })?;
        (form, Some(modrm))
    } else {
        (first, None)
    };
    let operand_size = state.prefixes.selection().operand_size(default_size);
    let address_size = state.prefixes.selection().address_size(default_size);
    let form = form
        .resolve(&state.prefixes, operand_size)
        .expect("the prefix state admits this form");
    let mut fields = match form.encoding().operands {
        OperandEncoding::None => DecodedFields::default(),
        OperandEncoding::OpcodeRegister => DecodedFields {
            register: Some(RegisterCode::from_bits(opcode)),
            ..DecodedFields::default()
        },
        OperandEncoding::ModRm => cursor.modrm_fields(
            address_size,
            modrm.expect("the selected encoding has ModRM"),
        )?,
        OperandEncoding::AbsoluteOffset => DecodedFields {
            absolute_offset: Some(cursor.integer(match address_size {
                AddressSize::Bits16 => FieldWidth::Word,
                AddressSize::Bits32 => FieldWidth::Dword,
            })?),
            ..DecodedFields::default()
        },
    };
    for (value, width) in fields.immediates.iter_mut().zip(form.encoding().immediates) {
        *value = width
            .map(|width| cursor.immediate(&form, width))
            .transpose()?;
    }
    let next_eip = instruction_eip.wrapping_add(cursor.offset as u32);
    Ok((
        SnapshotInstruction {
            form,
            address_size,
            fields,
            segment_override,
            eip: instruction_eip,
            next_eip,
            code_default_size: default_size,
        },
        &bytes[cursor.offset..],
    ))
}

struct SnapshotCursor<'a> {
    bytes: &'a [u8],
    instruction_eip: u32,
    offset: usize,
}

impl SnapshotCursor<'_> {
    fn byte(&mut self) -> Result<u8, BlockError> {
        if self.offset >= MAX_INSTRUCTION_BYTES as usize {
            return Err(BlockError::InstructionTooLong {
                address: self.instruction_eip,
            });
        }
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or(BlockError::TruncatedInstruction {
                address: self.instruction_eip,
                available: self.bytes.len(),
            })?;
        self.offset += 1;
        Ok(value)
    }

    fn integer(&mut self, width: FieldWidth) -> Result<u32, BlockError> {
        // Consume required bytes in order: missing bytes before the length limit
        // report truncation, while byte sixteen is never requested.
        let mut bits = 0;
        for offset in 0..width.bytes() {
            bits |= u32::from(self.byte()?) << (offset * 8);
        }
        Ok(bits)
    }

    fn immediate(&mut self, form: &ResolvedForm, field: ImmediateWidth) -> Result<u32, BlockError> {
        let bits = self.integer(form.immediate_width(field))?;
        Ok(if field.is_signed() {
            bits as u8 as i8 as i32 as u32
        } else {
            bits
        })
    }

    fn modrm_fields(
        &mut self,
        address_size: AddressSize,
        modrm: u8,
    ) -> Result<DecodedFields<u32, RegisterCode, SnapshotRm>, BlockError> {
        let rm = if modrm >> 6 == 3 {
            SnapshotRm::Register(RegisterCode::from_bits(modrm))
        } else {
            SnapshotRm::Memory(self.decode_address(modrm, address_size)?)
        };
        Ok(DecodedFields {
            modrm: Some(u32::from(modrm)),
            rm_index: Some(u32::from(modrm & 7)),
            register: Some(RegisterCode::from_bits(modrm >> 3)),
            rm: Some(rm),
            ..DecodedFields::default()
        })
    }
}
