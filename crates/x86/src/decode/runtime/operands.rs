//! Decodes operand layouts after the opcode has been read.
//! ModRM extensions are checked before fetching address bytes. Register operands
//! continue locally; memory operands transfer to a separate decoder entry.

use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    instruction::{
        forms_by_opcode, modrm_forms, DecodedFields, DecodedInstruction, Encoding, Form, Location,
        OpcodeMap, ResolvedForm,
    },
    register::RegisterCode,
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

impl<C> RuntimeDecoder<'_, C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    /// Decodes an opcode-selected register or accumulator followed by an immediate.
    pub(super) fn decode_immediate_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        opcode: &Val<I8>,
        form: &ResolvedForm,
    ) -> Result<(), BuildError> {
        let bits = cursor.immediate(&mut body, form)?;
        let fields = match form.encoding {
            Encoding::OpcodeRegisterImmediate => DecodedFields::OpcodeRegisterImmediate {
                register: RegisterCode::indexed(opcode.unsigned().extend::<I32>()),
                immediate: bits,
            },
            Encoding::AccumulatorImmediate => {
                DecodedFields::AccumulatorImmediate { immediate: bits }
            }
            _ => unreachable!("the selected form has an immediate and no ModRM"),
        };
        let decoded_instruction =
            form.bind(fields, cursor.instruction_eip().clone(), cursor.next_eip());
        (self.complete_instruction)(body, decoded_instruction)
    }

    pub(super) fn decode_accumulator_offset_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        form: &ResolvedForm,
    ) -> Result<(), BuildError> {
        let offset = cursor.dword(&mut body)?;
        let instruction = form.bind(
            DecodedFields::AccumulatorOffset { offset },
            cursor.instruction_eip().clone(),
            cursor.next_eip(),
        );
        (self.complete_instruction)(body, instruction)
    }

    pub(super) fn decode_modrm_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        opcode: &Val<I8>,
        forms: &[&'static Form],
    ) -> Result<(), BuildError> {
        // Opcode selection is complete; only its extension remains to be checked.
        let modrm = cursor.byte(&mut body)?;
        dispatch_form_by_extension(body, &modrm, forms, &|mut arm, form| {
            let Some(form) = form else {
                return cursor.return_unsupported(arm, opcode);
            };
            arm.if_(modrm.unsigned().shr(6).ne(3), |memory_body| {
                self.tail_call_memory_decoder(memory_body, &cursor, opcode, &modrm)
            })?;
            self.decode_register_operands(arm, cursor.clone(), &modrm, form)
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

    fn decode_register_operands(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        modrm: &Val<I8>,
        form: &Form,
    ) -> Result<(), BuildError> {
        let form = form.resolve(cursor.operand_size());
        let rm = Location::Register(RegisterCode::indexed(modrm.unsigned().extend::<I32>()));
        let fields = cursor.modrm_fields(&mut body, &form, modrm, rm)?;
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
                dispatch_form_by_extension(arm, modrm, forms, &|mut arm, form| {
                    let Some(form) = form else {
                        return cursor.return_unsupported(arm, opcode);
                    };
                    let form = form.resolve(cursor.operand_size());
                    let mut form_cursor = cursor.clone();
                    let fields = form_cursor.modrm_fields(
                        &mut arm,
                        &form,
                        modrm,
                        Location::Memory(address.clone()),
                    )?;
                    let instruction = form.bind(
                        fields,
                        form_cursor.instruction_eip().clone(),
                        form_cursor.next_eip(),
                    );
                    (self.complete_instruction)(arm, instruction)
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
    if !matches!(forms[0].encoding, Encoding::RmImmediate { .. }) {
        return continue_decoding(body, Some(forms[0]));
    }
    let extensions: BTreeMap<_, _> = forms
        .iter()
        .map(|form| {
            let Encoding::RmImmediate { extension, .. } = form.encoding else {
                unreachable!("one opcode has one physical ModRM layout")
            };
            (u32::from(extension), *form)
        })
        .collect();
    let keys: Vec<_> = extensions.keys().copied().collect();
    body.switch(modrm.unsigned().shr(3).and(7), &keys, |arm, key| {
        continue_decoding(arm, key.and_then(|key| extensions.get(&key)).copied())
    })?;
    body.trap()
}
