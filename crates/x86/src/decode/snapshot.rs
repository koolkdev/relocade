mod address;

use super::DecodeState;

use crate::{
    instruction::{
        DecodedFields, DecodedInstruction, FieldWidth, ImmediateWidth, Location, OperandEncoding,
        Prefix, PrefixState, ResolvedForm, EXTENDED_OPCODE_ESCAPE, MAX_INSTRUCTION_BYTES,
    },
    register::RegisterCode,
    segment::SegmentDefaultSize,
    BlockError,
};

pub(crate) fn snapshot(
    bytes: &[u8],
    instruction_eip: u32,
    default_size: SegmentDefaultSize,
) -> Result<(DecodedInstruction<u32, u32>, &[u8]), BlockError> {
    let mut cursor = SnapshotCursor {
        bytes,
        instruction_eip,
        offset: 0,
    };
    let mut state = DecodeState {
        prefixes: PrefixState::new(default_size),
        ..DecodeState::default()
    };
    let mut opcode = loop {
        let byte = cursor.byte()?;
        if let Some(prefix) = Prefix::from_byte(byte) {
            state = state.with_prefix(prefix);
        } else {
            break byte;
        }
    };
    let reported_opcode = state.unsupported_opcode_override().unwrap_or(opcode);
    if opcode == EXTENDED_OPCODE_ESCAPE {
        state = state.extended().ok_or(BlockError::UnsupportedInstruction {
            address: instruction_eip,
            opcode: reported_opcode,
        })?;
        opcode = cursor.byte()?;
    }
    let mut candidates = state.forms().filter(|form| form.matches(opcode));
    let first = candidates
        .next()
        .ok_or(BlockError::UnsupportedInstruction {
            address: instruction_eip,
            opcode: reported_opcode,
        })?;
    let (form, modrm) = if first.encoding.has_modrm() {
        let modrm = cursor.byte()?;
        let form = std::iter::once(first)
            .chain(candidates)
            .find(|form| form.matches_modrm(modrm, &state.prefixes))
            .ok_or(BlockError::UnsupportedInstruction {
                address: instruction_eip,
                opcode: reported_opcode,
            })?;
        (form, Some(modrm))
    } else {
        (first, None)
    };
    let form = form
        .resolve(&state.prefixes)
        .expect("the prefix state admits this form");
    let mut fields = match form.encoding().operands {
        OperandEncoding::None => DecodedFields::default(),
        OperandEncoding::OpcodeRegister => DecodedFields {
            register: Some(RegisterCode::from_code(opcode)),
            ..DecodedFields::default()
        },
        OperandEncoding::ModRm => {
            cursor.modrm_fields(&form, modrm.expect("the selected encoding has ModRM"))?
        }
        OperandEncoding::AbsoluteOffset => DecodedFields {
            absolute_offset: Some(cursor.integer(form.address_width())?),
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
        form.bind(fields, instruction_eip, next_eip),
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
        form: &ResolvedForm,
        modrm: u8,
    ) -> Result<DecodedFields<u32>, BlockError> {
        let rm = if modrm >> 6 == 3 {
            Location::Register(RegisterCode::from_code(modrm).into())
        } else {
            Location::Memory(
                self.decode_address(modrm, form.address_size())?
                    .memory()
                    .into(),
            )
        };
        Ok(DecodedFields {
            modrm: Some(u32::from(modrm)),
            rm_index: Some(u32::from(modrm & 7)),
            register: Some(RegisterCode::from_code(modrm >> 3)),
            rm: Some(rm),
            ..DecodedFields::default()
        })
    }
}
