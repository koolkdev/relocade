//! Decodes operand layouts after the opcode has been read.
//! ModRM extensions are checked before fetching address bytes. Register operands
//! continue locally; memory operands transfer to a separate decoder entry.

use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    decode::DecodeState,
    instruction::{
        forms_by_opcode, DecodedFields, DecodedInstruction, Encoding, Form, Location, OpcodeMap,
        ResolvedForm,
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
        form: &ResolvedForm,
    ) -> Result<(), BuildError> {
        let fields = match form.encoding() {
            Encoding::OpcodeOnly => DecodedFields::OpcodeOnly,
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
        state: DecodeState,
        opcode: &Val<I8>,
        forms: &[&'static Form],
    ) -> Result<(), BuildError> {
        // Select the extension, then validate the addressing mode before binding.
        let modrm = cursor.byte(&mut body)?;
        dispatch_form_by_extension(body, &modrm, forms, &|mut arm, form| {
            let Some(form) = form else {
                return state.return_unsupported(arm, &cursor, opcode);
            };
            arm.if_(modrm.unsigned().shr(6).ne(3), |memory_body| {
                self.tail_call_memory_decoder(memory_body, &cursor, state.clone(), opcode, &modrm)
            })?;
            if !form.accepts_register_rm() {
                return state.return_unsupported(arm, &cursor, opcode);
            }
            let rm =
                Location::Register(RegisterCode::indexed(modrm.unsigned().extend::<I32>()).into());
            let form = form
                .resolve(&state.prefixes)
                .expect("opcode selection accepted the prefix state");
            self.complete_modrm_instruction(arm, cursor.clone(), &modrm, &form, rm)
        })
    }

    fn tail_call_memory_decoder(
        &self,
        body: FunctionBuilder<'_>,
        cursor: &RuntimeCursor<'_>,
        state: DecodeState,
        opcode: &Val<I8>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let handlers = match state.map {
            OpcodeMap::Primary => &self.primary_modrm_memory_handlers,
            OpcodeMap::Extended => &self.extended_modrm_memory_handlers,
        };
        handlers.tail_call(body, cursor, state, &[opcode.into(), modrm.into()])
    }

    fn complete_modrm_instruction(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        modrm: &Val<I8>,
        form: &ResolvedForm,
        rm: Location<Val<I32>>,
    ) -> Result<(), BuildError> {
        let Encoding::ModRm { immediate } = form.encoding() else {
            unreachable!("the selected form has a ModRM field");
        };
        let immediate = immediate
            .map(|_| cursor.immediate(&mut body, form))
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
        state: DecodeState,
        opcode: &Val<I8>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let forms = forms_by_opcode(state.forms().filter(|form| form.encoding.has_modrm()));
        let opcodes: Vec<_> = forms.keys().copied().collect();
        super::address::decode(body, cursor, modrm, |mut body, cursor, address| {
            body.switch(opcode, &opcodes, |arm, key| {
                let Some(forms) = key.and_then(|key| forms.get(&key)) else {
                    return state.return_unsupported(arm, &cursor, opcode);
                };
                dispatch_form_by_extension(arm, modrm, forms, &|arm, form| {
                    let Some(form) = form else {
                        return state.return_unsupported(arm, &cursor, opcode);
                    };
                    let form = form
                        .resolve(&state.prefixes)
                        .expect("opcode selection accepted the prefix state");
                    self.complete_modrm_instruction(
                        arm,
                        cursor.clone(),
                        modrm,
                        &form,
                        Location::Memory(address.clone().memory().into()),
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
