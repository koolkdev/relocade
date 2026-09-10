//! Decodes operand layouts after the opcode has been read.
//! ModRM extensions are checked before fetching address bytes. Register operands
//! continue locally; memory operands transfer to a separate decoder entry.

use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    instruction::{
        forms_by_opcode, modrm_forms, DecodedFields, DecodedInstruction, Encoding, Form, Location,
        OpcodeMap, SizedForm,
    },
    register::RegisterCode,
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

impl<C> RuntimeDecoder<'_, C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    /// Decodes fields after exact opcode selection for forms without ModRM.
    pub(super) fn decode_opcode_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        opcode: u8,
        form: &SizedForm,
    ) -> Result<(), BuildError> {
        let fields = match form.encoding() {
            Encoding::OpcodeRegister => DecodedFields::OpcodeRegister {
                register: RegisterCode::from_code(opcode),
            },
            Encoding::OpcodeRegisterImmediate { .. } => DecodedFields::OpcodeRegisterImmediate {
                register: RegisterCode::from_code(opcode),
                immediate: cursor.immediate(&mut body, form)?,
            },
            Encoding::Immediate { .. } => DecodedFields::Immediate {
                immediate: cursor.immediate(&mut body, form)?,
            },
            Encoding::AccumulatorOffset => DecodedFields::AccumulatorOffset {
                offset: cursor.dword(&mut body)?,
            },
            _ => unreachable!("the selected form has no ModRM"),
        };
        let decoded_instruction =
            form.bind(fields, cursor.instruction_eip().clone(), cursor.next_eip());
        (self.complete_instruction)(body, decoded_instruction)
    }

    pub(super) fn decode_modrm_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        opcode: &Val<I8>,
        forms: &[&'static Form],
    ) -> Result<(), BuildError> {
        // Select the extension, then validate the addressing mode before binding.
        let modrm = cursor.byte(&mut body)?;
        dispatch_form_by_extension(body, &modrm, forms, &|mut arm, form| {
            let Some(form) = form else {
                return cursor.return_unsupported(arm, opcode);
            };
            arm.if_(modrm.unsigned().shr(6).ne(3), |memory_body| {
                self.tail_call_memory_decoder(memory_body, &cursor, opcode, &modrm)
            })?;
            if !form.accepts_register_rm() {
                return cursor.return_unsupported(arm, opcode);
            }
            let rm = Location::Register(RegisterCode::indexed(modrm.unsigned().extend::<I32>()));
            self.complete_modrm_instruction(arm, cursor.clone(), &modrm, form, rm)
        })
    }

    fn tail_call_memory_decoder(
        &self,
        body: FunctionBuilder<'_>,
        cursor: &RuntimeCursor<'_>,
        opcode: &Val<I8>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let handlers = match cursor.opcode_map() {
            OpcodeMap::Primary => &self.primary_modrm_memory_handlers,
            OpcodeMap::Extended => &self.extended_modrm_memory_handlers,
        };
        handlers.tail_call(body, cursor, &[opcode.into(), modrm.into()])
    }

    fn complete_modrm_instruction(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        modrm: &Val<I8>,
        form: &Form,
        rm: Location<Val<I32>>,
    ) -> Result<(), BuildError> {
        let form = form.with_operand_size(cursor.operand_size());
        let Encoding::ModRm { immediate } = form.encoding() else {
            unreachable!("the selected form has a ModRM field");
        };
        let immediate = immediate
            .map(|_| cursor.immediate(&mut body, &form))
            .transpose()?;
        let fields = DecodedFields::ModRm {
            register: RegisterCode::indexed(modrm.unsigned().shr(3).unsigned().extend::<I32>()),
            rm,
            immediate,
        };
        let instruction = form.bind(fields, cursor.instruction_eip().clone(), cursor.next_eip());
        (self.complete_instruction)(body, instruction)
    }

    /// Decodes address fields after the caller has accepted the opcode and any
    /// required ModRM extension and established a memory addressing mode.
    pub(super) fn decode_memory_operands(
        &self,
        body: FunctionBuilder<'_>,
        cursor: RuntimeCursor<'_>,
        opcode: &Val<I8>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let forms = forms_by_opcode(modrm_forms(cursor.opcode_map()));
        let opcodes: Vec<_> = forms.keys().copied().collect();
        super::address::decode(body, cursor, modrm, |mut body, cursor, address| {
            body.switch(opcode, &opcodes, |arm, key| {
                let Some(forms) = key.and_then(|key| forms.get(&key)) else {
                    return cursor.return_unsupported(arm, opcode);
                };
                dispatch_form_by_extension(arm, modrm, forms, &|arm, form| {
                    let Some(form) = form else {
                        return cursor.return_unsupported(arm, opcode);
                    };
                    self.complete_modrm_instruction(
                        arm,
                        cursor.clone(),
                        modrm,
                        form,
                        Location::Memory(address.clone()),
                    )
                })
            })?;
            body.trap()
        })
    }
}

/// Calls the continuation with the matching form, or `None` if unsupported.
/// Forms are nonempty and belong to one opcode; non-group forms ignore extension bits.
/// Memory entries repeat selection after address decoding because they receive
/// the opcode and ModRM rather than a selected form.
fn dispatch_form_by_extension(
    mut body: FunctionBuilder<'_>,
    modrm: &Val<I8>,
    forms: &[&'static Form],
    continue_decoding: &impl Fn(FunctionBuilder<'_>, Option<&Form>) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    if forms[0].extension.is_none() {
        return continue_decoding(body, Some(forms[0]));
    }
    let extensions: BTreeMap<_, _> = forms
        .iter()
        .map(|form| {
            let extension = form
                .extension
                .expect("group forms select ModRM.reg extensions");
            (u32::from(extension), *form)
        })
        .collect();
    let keys: Vec<_> = extensions.keys().copied().collect();
    body.switch(modrm.unsigned().shr(3).and(7), &keys, |arm, key| {
        continue_decoding(arm, key.and_then(|key| extensions.get(&key)).copied())
    })?;
    body.trap()
}
