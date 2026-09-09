use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::instruction::{
    forms_by_opcode, opcode_forms, DecodedInstruction, Encoding, Form, OpcodeMap,
    EXTENDED_OPCODE_ESCAPE, OPERAND_SIZE_PREFIX,
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

enum OpcodeAction {
    Instruction(Vec<&'static Form>),
    ExtendedMap,
    OperandSizePrefix,
}

impl<C> RuntimeDecoder<C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(super) fn decode_opcode(
        &self,
        mut body: FunctionBuilder<'_>,
        cursor: RuntimeCursor,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        let mut actions: BTreeMap<_, _> = forms_by_opcode(opcode_forms(cursor.opcode_map()))
            .into_iter()
            .map(|(opcode, forms)| (opcode, OpcodeAction::Instruction(forms)))
            .collect();
        if cursor.opcode_map() == OpcodeMap::Primary {
            actions.insert(u32::from(EXTENDED_OPCODE_ESCAPE), OpcodeAction::ExtendedMap);
            actions.insert(
                u32::from(OPERAND_SIZE_PREFIX),
                OpcodeAction::OperandSizePrefix,
            );
        }
        let opcodes: Vec<_> = actions.keys().copied().collect();
        body.switch(opcode, &opcodes, |mut arm, key| {
            match key.and_then(|key| actions.get(&key)) {
                Some(OpcodeAction::Instruction(forms)) => {
                    let form = forms[0];
                    match form.encoding {
                        Encoding::RegisterRm { .. }
                        | Encoding::RmImmediate { .. }
                        | Encoding::Rm => {
                            self.decode_modrm_operands(arm, cursor.clone(), opcode, forms)
                        }
                        Encoding::OpcodeRegisterImmediate | Encoding::AccumulatorImmediate => self
                            .decode_immediate_operands(
                                arm,
                                cursor.clone(),
                                opcode,
                                &form.resolve(cursor.operand_size()),
                            ),
                        Encoding::AccumulatorOffset { .. } => self
                            .decode_accumulator_offset_operands(
                                arm,
                                cursor.clone(),
                                &form.resolve(cursor.operand_size()),
                            ),
                    }
                }
                Some(OpcodeAction::ExtendedMap) => {
                    let mut extended = cursor.clone();
                    extended.enter_extended_map();
                    let selector = extended.byte(&mut arm)?;
                    self.decode_opcode(arm, extended, &selector)
                }
                Some(OpcodeAction::OperandSizePrefix) => {
                    let mut prefixed = cursor.clone();
                    prefixed.select_word_operands();
                    let opcode = prefixed.byte(&mut arm)?;
                    self.opcode_handlers
                        .tail_call(arm, &prefixed, &[(&opcode).into()])
                }
                None => cursor.return_unsupported(arm, opcode),
            }
        })?;
        body.trap()
    }
}
