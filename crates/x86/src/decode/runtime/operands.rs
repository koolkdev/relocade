use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    address::{Address32, IndexTerm, RegisterTerm},
    instruction::{
        DecodedFields, DecodedInstruction, Form, Location, OperandSize, ResolvedForm,
        ACCUMULATOR_OFFSET_FORMS, MODRM_FORMS,
    },
    register::{Register, RegisterCode},
    state::exit,
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

impl<C> RuntimeDecoder<C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(super) fn decode_immediate(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
        form: &ResolvedForm,
    ) -> Result<(), BuildError> {
        let bits = cursor.immediate(&mut body, form.width)?;
        let register = RegisterCode::indexed(opcode.unsigned().extend::<I32>());
        let decoded_instruction = form.bind(
            DecodedFields::OpcodeRegisterImmediate {
                register,
                immediate: bits,
            },
            cursor.instruction_eip().clone(),
            cursor.next_eip(),
        );
        (self.complete_instruction)(body, decoded_instruction)
    }

    pub(super) fn decode_absolute_offset(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        let offset = cursor.dword(&mut body)?;
        for form in &ACCUMULATOR_OFFSET_FORMS {
            body.if_(form.matches_value(opcode), |form_body| {
                let instruction = form.resolve(cursor.operand_size()).bind(
                    DecodedFields::AccumulatorOffset {
                        offset: offset.clone(),
                    },
                    cursor.instruction_eip().clone(),
                    cursor.next_eip(),
                );
                (self.complete_instruction)(form_body, instruction)
            })?;
        }
        body.return_(exit::unsupported(cursor.instruction_eip(), opcode))
    }

    pub(super) fn decode_modrm(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
        forms: impl Iterator<Item = &'static Form> + Clone,
    ) -> Result<(), BuildError> {
        // The entry has selected a ModRM form. Read its shared encoding before
        // binding the register and r/m fields to their semantic roles.
        let modrm = cursor.byte(&mut body)?;
        for form in forms.clone() {
            if let Some(mismatch) = form.encoding.extension_mismatch(&modrm) {
                body.if_(
                    form.matches_value(opcode).and(mismatch),
                    |unsupported_body| {
                        unsupported_body
                            .return_(exit::unsupported(cursor.instruction_eip(), opcode))
                    },
                )?;
            }
        }
        body.if_(
            modrm.unsigned().shr(6).ne(3),
            |memory_operand_body| match cursor.operand_size() {
                OperandSize::Dword => memory_operand_body.tail_call(
                    self.modrm_memory_handler,
                    &[
                        cursor.instruction_eip().into(),
                        opcode.into(),
                        (&modrm).into(),
                    ],
                ),
                OperandSize::Word => memory_operand_body.tail_call(
                    self.prefixed_modrm_memory_handler,
                    &[
                        cursor.instruction_eip().into(),
                        opcode.into(),
                        (&modrm).into(),
                        cursor.consumed().into(),
                    ],
                ),
            },
        )?;
        for form in forms {
            body.if_(form.matches_value(opcode), |mut form_body| {
                let form = form.resolve(cursor.operand_size());
                let mut form_cursor = cursor.clone();
                let rm =
                    Location::Register(RegisterCode::indexed(modrm.unsigned().extend::<I32>()));
                let fields = form_cursor.modrm_fields(&mut form_body, &form, &modrm, rm)?;
                let instruction = form.bind(
                    fields,
                    form_cursor.instruction_eip().clone(),
                    form_cursor.next_eip(),
                );
                (self.complete_instruction)(form_body, instruction)
            })?;
        }
        body.return_(exit::unsupported(cursor.instruction_eip(), opcode))
    }

    pub(super) fn decode_modrm_memory(
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
            body.if_(form.matches_value(opcode), |mut form_body| {
                let address = Address32 {
                    base: Some(RegisterTerm {
                        register: Register::<I32>::indexed(base.clone()),
                        present: Some(no_base.eq(0)),
                    }),
                    index: Some(IndexTerm {
                        register: RegisterTerm {
                            register: Register::<I32>::indexed(
                                sib.unsigned().shr(3).unsigned().extend::<I32>(),
                            ),
                            present: Some(has_sib.and(sib.unsigned().shr(3).and(7).ne(4))),
                        },
                        shift: sib.unsigned().shr(6).unsigned().extend::<I32>(),
                    }),
                    displacement: displacement.clone(),
                };
                let form = form.resolve(cursor.operand_size());
                let mut form_cursor = cursor.clone();
                let fields = form_cursor.modrm_fields(
                    &mut form_body,
                    &form,
                    modrm,
                    Location::Memory(address),
                )?;
                let instruction = form.bind(
                    fields,
                    form_cursor.instruction_eip().clone(),
                    form_cursor.next_eip(),
                );
                (self.complete_instruction)(form_body, instruction)
            })?;
        }
        body.return_(exit::unsupported(cursor.instruction_eip(), opcode))
    }
}
