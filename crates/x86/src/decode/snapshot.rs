use crate::{
    address::{Address32, IndexTerm, RegisterTerm},
    instruction::{
        DecodedFields, DecodedInstruction, Encoding, Location, OperandSize, OperandWidth,
        ResolvedForm, ACCUMULATOR_OFFSET_FORMS, MAX_INSTRUCTION_BYTES, MODRM_FORMS,
        OPCODE_REGISTER_IMMEDIATE_FORMS, OPERAND_SIZE_PREFIX,
    },
    register::{Gpr32, RegisterCode},
    BlockError,
};

pub(crate) fn snapshot(
    bytes: &[u8],
    instruction_eip: u32,
) -> Result<(DecodedInstruction<u32, u32>, &[u8]), BlockError> {
    let mut cursor = SnapshotCursor {
        bytes,
        instruction_eip,
        offset: 0,
    };
    let mut operand_size = OperandSize::Dword;
    let opcode = loop {
        let byte = cursor.byte()?;
        if byte != OPERAND_SIZE_PREFIX {
            break byte;
        }
        // Repeating the override preserves the selected size; it does not toggle it.
        operand_size = OperandSize::Word;
    };
    let form = OPCODE_REGISTER_IMMEDIATE_FORMS
        .iter()
        .chain(MODRM_FORMS.iter())
        .chain(ACCUMULATOR_OFFSET_FORMS.iter())
        .find(|form| form.matches(opcode))
        .ok_or(BlockError::UnsupportedInstruction {
            address: instruction_eip,
            opcode,
        })?
        .resolve(operand_size);
    let fields = match form.encoding {
        Encoding::OpcodeRegisterImmediate => DecodedFields::OpcodeRegisterImmediate {
            register: RegisterCode::from_code(opcode),
            immediate: cursor.integer(form.width)?,
        },
        Encoding::RegisterRm { .. } | Encoding::RmImmediate { .. } => {
            let modrm = cursor.byte()?;
            if !form.encoding.matches_modrm(modrm) {
                return Err(BlockError::UnsupportedInstruction {
                    address: instruction_eip,
                    opcode,
                });
            }
            cursor.modrm_fields(&form, modrm)?
        }
        Encoding::AccumulatorOffset { .. } => DecodedFields::AccumulatorOffset {
            offset: cursor.integer(OperandWidth::Dword)?,
        },
    };
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

    fn integer(&mut self, width: OperandWidth) -> Result<u32, BlockError> {
        // Consume required bytes in order: missing bytes before the length limit
        // report truncation, while byte sixteen is never requested.
        let mut bits = 0;
        for offset in 0..width.bytes() {
            bits |= u32::from(self.byte()?) << (offset * 8);
        }
        Ok(bits)
    }

    fn modrm_fields(
        &mut self,
        form: &ResolvedForm,
        modrm: u8,
    ) -> Result<DecodedFields<u32>, BlockError> {
        let rm = if modrm >> 6 == 3 {
            Location::Register(RegisterCode::from_code(modrm))
        } else {
            Location::Memory(self.decode_address(modrm)?)
        };
        Ok(match form.encoding {
            Encoding::RegisterRm { .. } => DecodedFields::RegisterRm {
                register: RegisterCode::from_code(modrm >> 3),
                rm,
            },
            Encoding::RmImmediate { .. } => DecodedFields::RmImmediate {
                rm,
                immediate: self.integer(form.width)?,
            },
            _ => unreachable!("the selected form has a ModRM field"),
        })
    }

    fn decode_address(&mut self, modrm: u8) -> Result<Address32<u32>, BlockError> {
        let mode = modrm >> 6;
        let rm = modrm & 7;
        let (base, index) = if rm == 4 {
            let sib = self.byte()?;
            let index = if (sib >> 3) & 7 == 4 {
                None
            } else {
                Some(IndexTerm {
                    register: named(sib >> 3),
                    shift: u32::from(sib >> 6),
                })
            };
            (sib & 7, index)
        } else {
            (rm, None)
        };
        let no_base = mode == 0 && base == 5;
        let displacement = if mode == 2 || no_base {
            self.integer(OperandWidth::Dword)?
        } else if mode == 1 {
            self.byte()? as i8 as i32 as u32
        } else {
            0
        };
        Ok(Address32 {
            base: (!no_base).then(|| named(base)),
            index,
            displacement,
        })
    }
}

fn named(code: u8) -> RegisterTerm {
    RegisterTerm {
        register: Gpr32::from_code(code).into(),
        present: None,
    }
}
