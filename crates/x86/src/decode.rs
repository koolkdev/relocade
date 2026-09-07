use wasm86_compiler::{BuildError, Func, FunctionBuilder, Program, Signature, Type, Val, I32, I8};

use crate::{
    fetch,
    instruction::{DecodedInstruction, Encoding, Form, Operand32, MOV_IMMEDIATE, REGISTER_FORMS},
    memory::{DirectRange, Memory},
    register::{Gpr32, Register32},
    state::exit,
    BlockError,
};

pub(super) fn snapshot(
    bytes: &[u8],
    address: u32,
) -> Result<(DecodedInstruction<u32, u32>, &[u8]), BlockError> {
    let Some(&opcode) = bytes.first() else {
        return Err(BlockError::TruncatedInstruction {
            address,
            available: 0,
        });
    };
    let form = std::iter::once(&MOV_IMMEDIATE)
        .chain(REGISTER_FORMS.iter())
        .find(|form| opcode & form.mask == form.opcode)
        .ok_or(BlockError::UnsupportedOpcode { address, opcode })?;
    let length = form.encoding.length() as usize;
    let instruction = bytes
        .get(..length)
        .ok_or(BlockError::TruncatedInstruction {
            address,
            available: bytes.len(),
        })?;
    let (register, operand) = match form.encoding {
        Encoding::OpcodeRegisterImmediate32 => {
            let immediate = u32::from_le_bytes(
                instruction[form.encoding.operand_offset() as usize..]
                    .try_into()
                    .unwrap(),
            );
            (
                Gpr32::from_code(opcode).into(),
                Operand32::Immediate(immediate),
            )
        }
        Encoding::ModRmRegister32 => {
            let modrm = instruction[form.encoding.operand_offset() as usize];
            if modrm >> 6 != 3 {
                return Err(BlockError::UnsupportedModRm {
                    address,
                    opcode,
                    modrm,
                });
            }
            (
                Gpr32::from_code(modrm >> 3).into(),
                Operand32::Register(Gpr32::from_code(modrm).into()),
            )
        }
    };
    let next_eip = address.wrapping_add(form.encoding.length());
    Ok((form.bind(register, operand, next_eip), &bytes[length..]))
}

/// Builds decoding code that reads guest instruction bytes during execution.
/// Separate handlers keep the entry graph small to discourage V8 from inlining
/// large checked-fetch code into the common path and adding stack spills.
pub(super) struct RuntimeDecoder {
    memory: Memory,
    exact: Func,
    direct: Func,
}

impl RuntimeDecoder {
    pub(super) fn new(
        program: &mut Program,
        memory: Memory,
        complete: impl Fn(
            FunctionBuilder<'_>,
            DecodedInstruction<Val<I32>, Val<I32>>,
        ) -> Result<(), BuildError>,
    ) -> Result<Self, BuildError> {
        let exact = program.declare(Signature {
            parameters: vec![Type::I32, Type::I8],
            result: Type::I64,
        });
        let direct = program.declare(Signature {
            parameters: vec![Type::I32, Type::I8, Type::I32],
            result: Type::I64,
        });
        let body = program.define(exact)?;
        let start = body.parameter::<I32>(0)?;
        let opcode = body.parameter::<I8>(1)?;
        non_immediate(body, memory, &start, None, &opcode, &complete)?;
        let body = program.define(direct)?;
        let start = body.parameter::<I32>(0)?;
        let opcode = body.parameter::<I8>(1)?;
        let physical = body.parameter::<I32>(2)?;
        non_immediate(body, memory, &start, Some(&physical), &opcode, complete)?;
        Ok(Self {
            memory,
            exact,
            direct,
        })
    }

    pub(super) fn direct_window(
        &self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
    ) -> Result<DirectRange, BuildError> {
        let bytes = std::iter::once(&MOV_IMMEDIATE)
            .chain(REGISTER_FORMS.iter())
            .map(|form| form.encoding.length())
            .max()
            .unwrap();
        self.memory.direct(body, start, bytes)
    }

    /// `physical` supplies a proven contiguous instruction window. The callback
    /// lowers and completes each selected path; decoded child values do not escape it.
    pub(super) fn decode(
        &self,
        mut body: FunctionBuilder<'_>,
        start: &Val<I32>,
        physical: Option<&Val<I32>>,
        complete: impl Fn(
            FunctionBuilder<'_>,
            DecodedInstruction<Val<I32>, Val<I32>>,
        ) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let opcode = match physical {
            Some(address) => self.memory.load::<I8>(&mut body, address, 0)?,
            None => fetch::byte(&mut body, self.memory, start)?,
        };
        let form = &MOV_IMMEDIATE;
        body.if_(
            opcode.and(u32::from(form.mask)).ne(u32::from(form.opcode)),
            |mut arm| {
                for form in &REGISTER_FORMS {
                    arm.if_(
                        opcode.and(u32::from(form.mask)).eq(u32::from(form.opcode)),
                        |selected| match physical {
                            Some(address) => selected.tail_call(
                                self.direct,
                                &[start.into(), (&opcode).into(), address.into()],
                            ),
                            None => {
                                selected.tail_call(self.exact, &[start.into(), (&opcode).into()])
                            }
                        },
                    )?;
                }
                arm.return_(exit::unsupported(start, &opcode))
            },
        )?;
        let decoded = runtime_operands(&mut body, self.memory, start, physical, &opcode, form)?;
        complete(body, decoded)
    }
}

fn non_immediate(
    mut body: FunctionBuilder<'_>,
    memory: Memory,
    start: &Val<I32>,
    physical: Option<&Val<I32>>,
    opcode: &Val<I8>,
    complete: impl Fn(
        FunctionBuilder<'_>,
        DecodedInstruction<Val<I32>, Val<I32>>,
    ) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    for form in &REGISTER_FORMS {
        body.if_(
            opcode.and(u32::from(form.mask)).eq(u32::from(form.opcode)),
            |mut selected| {
                let decoded =
                    runtime_operands(&mut selected, memory, start, physical, opcode, form)?;
                complete(selected, decoded)
            },
        )?;
    }
    body.return_(exit::unsupported(start, opcode))
}

fn runtime_operands(
    body: &mut FunctionBuilder<'_>,
    memory: Memory,
    start: &Val<I32>,
    physical: Option<&Val<I32>>,
    opcode: &Val<I8>,
    form: &Form,
) -> Result<DecodedInstruction<Val<I32>, Val<I32>>, BuildError> {
    let offset = form.encoding.operand_offset();
    let (register, operand) = match form.encoding {
        Encoding::OpcodeRegisterImmediate32 => {
            let immediate = match physical {
                Some(address) => memory.load::<I32>(body, address, offset)?,
                None => fetch::immediate32(body, memory, &start.add(offset))?,
            };
            (
                Register32::indexed(opcode.unsigned().extend::<I32>()),
                Operand32::Immediate(immediate),
            )
        }
        Encoding::ModRmRegister32 => {
            let modrm = match physical {
                Some(address) => memory.load::<I8>(body, address, offset)?,
                None => fetch::byte(body, memory, &start.add(offset))?,
            };
            body.if_(modrm.unsigned().shr(6).ne(3), |arm| {
                arm.return_(exit::unsupported(start, opcode))
            })?;
            let reg = Register32::indexed(modrm.unsigned().shr(3).unsigned().extend::<I32>());
            let rm = Register32::indexed(modrm.unsigned().extend::<I32>());
            (reg, Operand32::Register(rm))
        }
    };
    Ok(form.bind(register, operand, start.add(form.encoding.length())))
}
