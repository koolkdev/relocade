//! Decodes operand layouts after the opcode has been read.
//! ModRM forms are selected before fetching address bytes. Register operands
//! continue locally; memory operands join their shared address decoder.

use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32, I8};

use crate::{
    decode::DecodeState,
    instruction::{
        forms_by_opcode, DecodedFields, DecodedInstruction, Form, Location, OpcodeMap,
        OperandEncoding, OperandSize, PrefixState, ResolvedForm,
    },
    register::{RegisterCode, RegisterSelector},
    state::exit,
};

use super::{cursor::RuntimeCursor, selectors::dispatch_modrm_form, InstructionDecoder};

impl<C> InstructionDecoder<'_, C>
where
    C: Fn(BlockBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    fn address_dword(&self, prefixes: &PrefixState) -> Val<I1> {
        self.code_default_is_32_bit
            .xor(prefixes.selection().has_address_override())
    }

    fn address_mask(&self, prefixes: &PrefixState) -> Val<I32> {
        self.address_dword(prefixes)
            .select(u32::MAX, u32::from(u16::MAX))
    }

    /// Resolve a typed handler only where operand width changes decoding or effects.
    /// Mandatory 66 selection remains a property of the collected prefix bytes.
    pub(super) fn decode_form(
        &self,
        mut body: BlockBuilder<'_>,
        form: &Form,
        prefixes: &PrefixState,
        complete: impl Fn(BlockBuilder<'_>, ResolvedForm) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let resolve = |size| {
            form.resolve(prefixes, size)
                .expect("opcode selection accepted the prefix state")
        };
        if !form.uses_operand_size() {
            return complete(body, resolve(OperandSize::Dword));
        }
        let dword = self
            .code_default_is_32_bit
            .xor(prefixes.selection().has_operand_override());
        if let Some(dword) = body.constant_bits(&dword)? {
            return complete(
                body,
                resolve(if dword != 0 {
                    OperandSize::Dword
                } else {
                    OperandSize::Word
                }),
            );
        }
        body.if_else(
            dword,
            |arm| complete(arm, resolve(OperandSize::Dword)),
            |arm| complete(arm, resolve(OperandSize::Word)),
        )?;
        body.trap()
    }

    /// Decodes fields after exact opcode selection for forms without ModRM.
    pub(super) fn decode_opcode_operands(
        &self,
        mut body: BlockBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        opcode: u8,
        form: &ResolvedForm,
        prefixes: &PrefixState,
    ) -> Result<(), BuildError> {
        let mut fields = match form.encoding().operands {
            OperandEncoding::None => DecodedFields::default(),
            OperandEncoding::OpcodeRegister => DecodedFields {
                register: Some(RegisterSelector::Fixed(RegisterCode::from_bits(opcode))),
                ..DecodedFields::default()
            },
            OperandEncoding::AbsoluteOffset => DecodedFields {
                absolute_offset: Some(
                    cursor.address_offset(&mut body, &self.address_dword(prefixes))?,
                ),
                ..DecodedFields::default()
            },
            OperandEncoding::ModRm => unreachable!("the selected form has no ModRM"),
        };
        cursor.read_immediates(&mut body, form, &mut fields)?;
        let decoded_instruction = form.bind(
            fields,
            cursor.instruction_eip().clone(),
            cursor.next_eip(),
            prefixes.segment_override(),
            self.address_mask(prefixes),
        );
        (self.complete_instruction)(body, decoded_instruction)
    }

    pub(super) fn decode_modrm_operands(
        &self,
        mut body: BlockBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        state: DecodeState,
        opcode: u8,
        forms: &[&'static Form],
    ) -> Result<(), BuildError> {
        let modrm = cursor.byte(&mut body)?;
        let memory_forms = memory_forms(&state);
        dispatch_modrm_form(body, &modrm, forms, &state, &|mut arm, form| {
            let Some(form) = form else {
                return exit::unsupported(arm, cursor.instruction_eip());
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
                return exit::unsupported(arm, cursor.instruction_eip());
            }
            let rm = Location::Register(
                RegisterSelector::Runtime(modrm.unsigned().extend::<I32>()).into(),
            );
            self.decode_form(arm, form, &state.prefixes, |arm, form| {
                self.complete_modrm_instruction(
                    arm,
                    cursor.clone(),
                    &modrm,
                    &form,
                    rm.clone(),
                    &state.prefixes,
                )
            })
        })
    }

    fn continue_memory_decoding(
        &self,
        body: BlockBuilder<'_>,
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
        handlers.tail_call(
            body,
            cursor,
            state,
            &[form_index.into(), modrm.into()],
            &self.code_default_is_32_bit,
        )
    }

    fn complete_modrm_instruction(
        &self,
        mut body: BlockBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        modrm: &Val<I8>,
        form: &ResolvedForm,
        rm: Location<Val<I32>>,
        prefixes: &PrefixState,
    ) -> Result<(), BuildError> {
        let mut fields = DecodedFields {
            modrm: Some(modrm.unsigned().extend::<I32>()),
            rm_index: Some(modrm.and(7).unsigned().extend::<I32>()),
            rm: Some(rm),
            ..DecodedFields::default()
        };
        cursor.read_immediates(&mut body, form, &mut fields)?;
        fields.register = Some(RegisterSelector::Runtime(
            modrm.unsigned().shr(3).unsigned().extend::<I32>(),
        ));
        let instruction = form.bind(
            fields,
            cursor.instruction_eip().clone(),
            cursor.next_eip(),
            prefixes.segment_override(),
            self.address_mask(prefixes),
        );
        (self.complete_instruction)(body, instruction)
    }

    /// Decodes address fields after the caller has selected a form from its
    /// opcode and fixed ModRM bits and established a memory addressing mode.
    pub(super) fn decode_memory_operands(
        &self,
        mut body: BlockBuilder<'_>,
        mut cursor: RuntimeCursor<'_>,
        state: DecodeState,
        form_index: &Val<I32>,
        modrm: &Val<I8>,
    ) -> Result<(), BuildError> {
        let forms = memory_forms(&state);
        let indices: Vec<_> = (0..forms.len() as u32).collect();
        let address = super::address::decode(
            &mut body,
            &mut cursor,
            modrm,
            &self.address_dword(&state.prefixes),
        )?;
        let rm = Location::Memory(address.memory().into());
        body.switch(form_index, &indices, |arm, index| {
            let Some(index) = index else {
                return arm.trap();
            };
            self.decode_form(
                arm,
                forms[index as usize].form,
                &state.prefixes,
                |arm, form| {
                    self.complete_modrm_instruction(
                        arm,
                        cursor.clone(),
                        modrm,
                        &form,
                        rm.clone(),
                        &state.prefixes,
                    )
                },
            )
        })?;
        body.trap()
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
