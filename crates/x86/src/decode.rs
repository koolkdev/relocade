use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I1, I32, I8,
};

use crate::{
    address::{Address32, IndexTerm, RegisterTerm},
    fetch,
    instruction::{
        DecodedInstruction, Encoding, Location32, Operand32, MODRM_FORMS, MOV_IMMEDIATE,
    },
    memory::{DirectRange, Intent, Memory},
    register::{Gpr32, Register32},
    state::exit,
    BlockError,
};

pub(super) fn snapshot(
    bytes: &[u8],
    instruction_eip: u32,
) -> Result<(DecodedInstruction<u32, u32>, &[u8]), BlockError> {
    let Some(&opcode) = bytes.first() else {
        return Err(BlockError::TruncatedInstruction {
            address: instruction_eip,
            available: 0,
        });
    };
    let form = std::iter::once(&MOV_IMMEDIATE)
        .chain(MODRM_FORMS.iter())
        .find(|form| form.matches(opcode))
        .ok_or(BlockError::UnsupportedOpcode {
            address: instruction_eip,
            opcode,
        })?;
    let mut cursor = SnapshotCursor {
        bytes,
        instruction_eip,
        offset: form.encoding.operand_offset() as usize,
    };
    let (register, operand) = match form.encoding {
        Encoding::OpcodeRegisterImmediate32 => (
            Gpr32::from_code(opcode).into(),
            Operand32::Immediate(cursor.dword()?),
        ),
        Encoding::ModRm32 => {
            let modrm = cursor.byte()?;
            let register = Gpr32::from_code(modrm >> 3).into();
            let operand = if modrm >> 6 == 3 {
                Location32::Register(Gpr32::from_code(modrm).into())
            } else {
                Location32::Memory(cursor.decode_address(modrm)?)
            };
            (register, Operand32::Location(operand))
        }
    };
    let next_eip = instruction_eip.wrapping_add(cursor.offset as u32);
    Ok((
        form.bind(register, operand, instruction_eip, next_eip),
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

    fn dword(&mut self) -> Result<u32, BlockError> {
        let bytes = self.bytes.get(self.offset..self.offset + 4).ok_or(
            BlockError::TruncatedInstruction {
                address: self.instruction_eip,
                available: self.bytes.len(),
            },
        )?;
        self.offset += 4;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
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
            self.dword()?
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

/// Builds decoding code that reads guest instruction bytes during execution.
/// Separate handlers keep the entry graph small to discourage V8 from inlining
/// large checked-fetch code into the common path and adding stack spills.
pub(super) struct RuntimeDecoder<C> {
    memory: Memory,
    checked_modrm_handler: Func,
    direct_modrm_handler: Func,
    memory_operand_handler: Func,
    complete_instruction: C,
}

impl<C> RuntimeDecoder<C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(super) fn new(
        program: &mut Program,
        memory: Memory,
        complete_instruction: C,
    ) -> Result<Self, BuildError> {
        let checked_modrm_handler = program.declare(Signature {
            parameters: vec![Type::I32, Type::I8],
            result: Type::I64,
        });
        let direct_modrm_handler = program.declare(Signature {
            parameters: vec![Type::I32, Type::I8, Type::I32],
            result: Type::I64,
        });
        let memory_operand_handler = program.declare(Signature {
            parameters: vec![Type::I32, Type::I8, Type::I8],
            result: Type::I64,
        });
        let decoder = Self {
            memory,
            checked_modrm_handler,
            direct_modrm_handler,
            memory_operand_handler,
            complete_instruction,
        };

        let body = program.define(memory_operand_handler)?;
        let instruction_eip = body.parameter::<I32>(0)?;
        let opcode = body.parameter::<I8>(1)?;
        let modrm = body.parameter::<I8>(2)?;
        let cursor = RuntimeCursor::new(
            &body,
            memory,
            &instruction_eip,
            None,
            Encoding::ModRm32.minimum_length(),
        )?;
        decoder.decode_memory(body, cursor, &opcode, &modrm)?;

        let body = program.define(checked_modrm_handler)?;
        let instruction_eip = body.parameter::<I32>(0)?;
        let opcode = body.parameter::<I8>(1)?;
        let cursor = RuntimeCursor::new(
            &body,
            memory,
            &instruction_eip,
            None,
            Encoding::ModRm32.operand_offset(),
        )?;
        decoder.decode_modrm(body, cursor, &opcode)?;

        let body = program.define(direct_modrm_handler)?;
        let instruction_eip = body.parameter::<I32>(0)?;
        let opcode = body.parameter::<I8>(1)?;
        let physical_start = body.parameter::<I32>(2)?;
        let cursor = RuntimeCursor::new(
            &body,
            memory,
            &instruction_eip,
            Some(&physical_start),
            Encoding::ModRm32.operand_offset(),
        )?;
        decoder.decode_modrm(body, cursor, &opcode)?;
        Ok(decoder)
    }

    pub(super) fn direct_window(
        &self,
        body: &mut FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
    ) -> Result<DirectRange, BuildError> {
        // A memory operand handler checks any SIB/displacement suffix separately;
        // extending this common proof would burden shorter instruction forms.
        let bytes = MOV_IMMEDIATE.encoding.minimum_length();
        self.memory
            .check_direct_access(body, instruction_eip, bytes, Intent::Fetch)
    }

    /// `physical_start` supplies the proven contiguous instruction window. The stored
    /// completion policy consumes each selected path and its decoded operands.
    pub(super) fn decode(
        &self,
        mut body: FunctionBuilder<'_>,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
    ) -> Result<(), BuildError> {
        let mut cursor =
            RuntimeCursor::new(&body, self.memory, instruction_eip, physical_start, 0)?;
        let opcode = cursor.byte(&mut body)?;
        body.if_(
            MOV_IMMEDIATE.matches_value(&opcode).eq(0),
            |mut modrm_dispatch_body| {
                for form in &MODRM_FORMS {
                    modrm_dispatch_body.if_(form.matches_value(&opcode), |form_body| {
                        match physical_start {
                            Some(physical_start) => form_body.tail_call(
                                self.direct_modrm_handler,
                                &[
                                    instruction_eip.into(),
                                    (&opcode).into(),
                                    physical_start.into(),
                                ],
                            ),
                            None => form_body.tail_call(
                                self.checked_modrm_handler,
                                &[instruction_eip.into(), (&opcode).into()],
                            ),
                        }
                    })?;
                }
                modrm_dispatch_body.return_(exit::unsupported(instruction_eip, &opcode))
            },
        )?;
        let immediate = cursor.dword(&mut body)?;
        let register = Register32::indexed(opcode.unsigned().extend::<I32>());
        let decoded_instruction = MOV_IMMEDIATE.bind(
            register,
            Operand32::Immediate(immediate),
            cursor.instruction_eip.clone(),
            cursor.next_eip(),
        );
        (self.complete_instruction)(body, decoded_instruction)
    }

    fn decode_modrm(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        // The entry has selected a ModRM form. Read its shared encoding before
        // binding the register and r/m fields to their semantic roles.
        let modrm = cursor.byte(&mut body)?;
        body.if_(modrm.unsigned().shr(6).ne(3), |memory_operand_body| {
            memory_operand_body.tail_call(
                self.memory_operand_handler,
                &[
                    (&cursor.instruction_eip).into(),
                    opcode.into(),
                    (&modrm).into(),
                ],
            )
        })?;
        let register = modrm.unsigned().shr(3).unsigned().extend::<I32>();
        let rm = modrm.unsigned().extend::<I32>();
        let next_eip = cursor.next_eip();
        for form in &MODRM_FORMS {
            body.if_(form.matches_value(opcode), |form_body| {
                let decoded_instruction = form.bind(
                    Register32::indexed(register.clone()),
                    Operand32::Location(Location32::Register(Register32::indexed(rm.clone()))),
                    cursor.instruction_eip.clone(),
                    next_eip.clone(),
                );
                (self.complete_instruction)(form_body, decoded_instruction)
            })?;
        }
        body.return_(exit::unsupported(&cursor.instruction_eip, opcode))
    }

    fn decode_memory(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let mode = modrm.unsigned().shr(6);
        let rm = modrm.and(7).unsigned().extend::<I32>();
        let has_sib = rm.eq(4);
        let sib = cursor.optional_byte(&mut body, &has_sib)?;
        let base = has_sib.select(sib.and(7).unsigned().extend::<I32>(), &rm);
        let no_base = mode.eq(0).and(base.eq(5));
        let displacement = cursor.displacement(&mut body, &mode, &no_base)?;
        for form in &MODRM_FORMS {
            body.if_(form.matches_value(opcode), |form_body| {
                let address = Address32 {
                    base: Some(RegisterTerm {
                        register: Register32::indexed(base.clone()),
                        present: Some(no_base.eq(0)),
                    }),
                    index: Some(IndexTerm {
                        register: RegisterTerm {
                            register: Register32::indexed(
                                sib.unsigned().shr(3).unsigned().extend::<I32>(),
                            ),
                            present: Some(has_sib.and(sib.unsigned().shr(3).and(7).ne(4))),
                        },
                        shift: sib.unsigned().shr(6).unsigned().extend::<I32>(),
                    }),
                    displacement: displacement.clone(),
                };
                let register =
                    Register32::indexed(modrm.unsigned().shr(3).unsigned().extend::<I32>());
                let decoded_instruction = form.bind(
                    register,
                    Operand32::Location(Location32::Memory(address)),
                    cursor.instruction_eip.clone(),
                    cursor.next_eip(),
                );
                (self.complete_instruction)(form_body, decoded_instruction)
            })?;
        }
        body.return_(exit::unsupported(&cursor.instruction_eip, opcode))
    }
}

/// A proven window uses fixed displacements; a checked cursor advances in the
/// wrapping instruction address space. A read beyond the proven extent uses
/// checked fetch. Conditional fields discard the window and advance the parent
/// cursor by their selected size, never by retaining a child-scoped position.
#[derive(Clone)]
struct RuntimeCursor {
    memory: Memory,
    instruction_eip: Val<I32>,
    offset: Val<I32>,
    window: Option<Window>,
}

#[derive(Clone)]
struct Window {
    physical_start: Val<I32>,
    consumed: u32,
    bytes: u32,
}

impl Window {
    fn covers(&self, bytes: u32) -> bool {
        u64::from(self.consumed) + u64::from(bytes) <= u64::from(self.bytes)
    }
}

impl RuntimeCursor {
    fn new(
        body: &FunctionBuilder<'_>,
        memory: Memory,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
        consumed: u32,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            memory,
            instruction_eip: instruction_eip.clone(),
            offset: body.value(consumed)?,
            window: physical_start.map(|physical_start| Window {
                physical_start: physical_start.clone(),
                consumed,
                bytes: MOV_IMMEDIATE.encoding.minimum_length(),
            }),
        })
    }

    fn next_eip(&self) -> Val<I32> {
        self.instruction_eip.add(&self.offset)
    }

    fn byte(&mut self, body: &mut FunctionBuilder<'_>) -> Result<Val<I8>, BuildError> {
        let value = match &self.window {
            Some(window) if window.covers(1) => {
                self.memory
                    .load(body, &window.physical_start, window.consumed)?
            }
            _ => fetch::byte(body, self.memory, &self.next_eip())?,
        };
        self.advance(1);
        Ok(value)
    }

    fn dword(&mut self, body: &mut FunctionBuilder<'_>) -> Result<Val<I32>, BuildError> {
        let value = match &self.window {
            Some(window) if window.covers(4) => {
                self.memory
                    .load(body, &window.physical_start, window.consumed)?
            }
            _ => fetch::dword(body, self.memory, &self.next_eip())?,
        };
        self.advance(4);
        Ok(value)
    }

    fn advance(&mut self, bytes: u32) {
        self.offset = self.offset.add(bytes);
        if let Some(window) = &mut self.window {
            window.consumed += bytes;
        }
    }

    fn optional_byte(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        present: &Val<I1>,
    ) -> Result<Val<I8>, BuildError> {
        self.window = None;
        let mut conditional_cursor = self.clone();
        let value = body.if_value::<I8>(
            present,
            |mut field_body| {
                let value = conditional_cursor.byte(&mut field_body)?;
                field_body.yield_(value)
            },
            |absent_field_body| absent_field_body.yield_(0),
        )?;
        self.offset = self.offset.add(present.unsigned().extend::<I32>());
        Ok(value)
    }

    fn displacement(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        mode: &Val<I8>,
        no_base: &Val<I1>,
    ) -> Result<Val<I32>, BuildError> {
        self.window = None;
        let has_dword_displacement = mode.eq(2).or(no_base);
        let has_byte_displacement = mode.eq(1);
        let value = body.if_value::<I32>(
            &has_dword_displacement,
            |mut dword_body| {
                let value = self.clone().dword(&mut dword_body)?;
                dword_body.yield_(value)
            },
            |mut short_displacement_body| {
                let value = short_displacement_body.if_value::<I32>(
                    &has_byte_displacement,
                    |mut byte_body| {
                        let value = self.clone().byte(&mut byte_body)?;
                        byte_body.yield_(value.signed().extend::<I32>())
                    },
                    |no_displacement_body| no_displacement_body.yield_(0),
                )?;
                short_displacement_body.yield_(value)
            },
        )?;
        self.offset = self
            .offset
            .add(has_dword_displacement.unsigned().extend::<I32>().shl(2))
            .add(has_byte_displacement.unsigned().extend::<I32>());
        Ok(value)
    }
}
