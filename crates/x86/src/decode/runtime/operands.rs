//! Decodes operand layouts after the opcode has been read.
//! ModRM forms are selected before fetching address bytes. Register operands
//! continue locally; memory operands join their shared address decoder.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    decode::DecodeState,
    instruction::{
        forms_by_opcode, DecodedFields, DecodedInstruction, Form, Location, OpcodeMap,
        OperandEncoding, ResolvedForm,
    },
    register::RegisterCode,
};

use super::{cursor::RuntimeCursor, selectors::dispatch_modrm_form, InstructionDecoder};

impl<C> InstructionDecoder<'_, '_, C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    /// Decodes fields after exact opcode selection for forms without ModRM.
    pub(super) fn decode_opcode_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        opcode: u8,
        form: &ResolvedForm,
    ) -> Result<(), BuildError> {
        let mut fields = match form.encoding().operands {
            OperandEncoding::None => DecodedFields::default(),
            OperandEncoding::OpcodeRegister => DecodedFields {
                register: Some(RegisterCode::from_code(opcode)),
                ..DecodedFields::default()
            },
            OperandEncoding::AbsoluteOffset => DecodedFields {
                absolute_offset: Some(cursor.integer(&mut body, form.address_width())?),
                ..DecodedFields::default()
            },
            OperandEncoding::ModRm => unreachable!("the selected form has no ModRM"),
        };
        cursor.read_immediates(&mut body, form, &mut fields)?;
        let decoded_instruction =
            form.bind(fields, cursor.instruction_eip().clone(), cursor.next_eip());
        (self.complete_instruction)(body, decoded_instruction)
    }

    pub(super) fn decode_modrm_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        state: DecodeState,
        opcode: u8,
        forms: &[&'static Form],
    ) -> Result<(), BuildError> {
        let modrm = cursor.byte(&mut body)?;
        let opcode_value = body.value::<I8>(u32::from(opcode))?;
        let memory_forms = memory_forms(&state);
        dispatch_modrm_form(body, &modrm, forms, &state, &|mut arm, form| {
            let Some(form) = form else {
                return state.return_unsupported(arm, &cursor, &opcode_value);
            };
            if form.accepts_memory_rm() {
                let form_index = memory_forms
                    .iter()
                    .position(|candidate| {
                        candidate.opcode == opcode && std::ptr::eq(candidate.form, form)
                    })
                    .expect("the accepted memory form has a decoder index")
                    as u32;
                arm.if_(modrm.unsigned().shr(6).ne(3), |memory_body| {
                    self.continue_memory_decoding(
                        memory_body,
                        &cursor,
                        state.clone(),
                        form_index,
                        &modrm,
                    )
                })?;
            }
            if !form.accepts_register_rm(&state.prefixes) {
                return state.return_unsupported(arm, &cursor, &opcode_value);
            }
            let rm =
                Location::Register(RegisterCode::indexed(modrm.unsigned().extend::<I32>()).into());
            let form = form
                .resolve(&state.prefixes)
                .expect("opcode selection accepted the prefix state");
            self.complete_modrm_instruction(arm, cursor.clone(), &modrm, &form, rm)
        })
    }

    fn continue_memory_decoding(
        &self,
        body: FunctionBuilder<'_>,
        cursor: &RuntimeCursor<'_>,
        state: DecodeState,
        form_index: u32,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        if let Some(target) = &self.memory_continuation {
            return body.branch(
                target,
                (state.map == OpcodeMap::Extended, form_index, modrm),
            );
        }
        let handlers = match state.map {
            OpcodeMap::Primary => &self.decoder.primary_modrm_memory_handlers,
            OpcodeMap::Extended => &self.decoder.extended_modrm_memory_handlers,
        };
        handlers.tail_call(body, cursor, state, &[form_index.into(), modrm.into()])
    }

    fn complete_modrm_instruction(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        modrm: &Val<I8>,
        form: &ResolvedForm,
        rm: Location<Val<I32>>,
    ) -> Result<(), BuildError> {
        let mut fields = DecodedFields {
            modrm: Some(modrm.unsigned().extend::<I32>()),
            rm_index: Some(modrm.and(7).unsigned().extend::<I32>()),
            rm: Some(rm),
            ..DecodedFields::default()
        };
        cursor.read_immediates(&mut body, form, &mut fields)?;
        fields.register = Some(RegisterCode::indexed(
            modrm.unsigned().shr(3).unsigned().extend::<I32>(),
        ));
        let instruction = form.bind(fields, cursor.instruction_eip().clone(), cursor.next_eip());
        (self.complete_instruction)(body, instruction)
    }

    /// Decodes address fields after the caller has selected a form from its
    /// opcode and fixed ModRM bits and established a memory addressing mode.
    pub(super) fn decode_memory_operands(
        &self,
        body: FunctionBuilder<'_>,
        cursor: RuntimeCursor<'_>,
        state: DecodeState,
        form_index: &Val<I32>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let forms = memory_forms(&state);
        let indices: Vec<_> = (0..forms.len() as u32).collect();
        super::address::decode(
            body,
            cursor,
            modrm,
            state.prefixes.address_size(),
            |mut body, cursor, address| {
                body.switch(form_index, &indices, |arm, index| {
                    let Some(index) = index else {
                        return arm.trap();
                    };
                    let form = forms[index as usize]
                        .form
                        .resolve(&state.prefixes)
                        .expect("opcode selection accepted the prefix state");
                    self.complete_modrm_instruction(
                        arm,
                        cursor.clone(),
                        modrm,
                        &form,
                        Location::Memory(address.clone().memory().into()),
                    )
                })?;
                body.trap()
            },
        )
    }
}

struct MemoryForm {
    opcode: u8,
    form: &'static Form,
}

/// A prefix state's accepted memory forms have dense indices shared by the
/// initial selection and the continuation after address decoding.
fn memory_forms(state: &DecodeState) -> Vec<MemoryForm> {
    forms_by_opcode(state.forms().filter(|form| form.accepts_memory_rm()))
        .into_iter()
        .flat_map(|(opcode, forms)| {
            forms.into_iter().map(move |form| MemoryForm {
                opcode: opcode as u8,
                form,
            })
        })
        .collect()
}
