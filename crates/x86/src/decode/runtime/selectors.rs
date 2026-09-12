use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::instruction::{
    forms_by_opcode, opcode_forms, DecodedInstruction, Form, OpcodeMap, EXTENDED_OPCODE_ESCAPE,
    OPERAND_SIZE_PREFIX, REPEAT_PREFIX,
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

enum OpcodeAction {
    Instruction(Vec<&'static Form>),
    ExtendedMap,
    OperandSizePrefix,
    RepeatPrefix,
}

impl<C> RuntimeDecoder<'_, C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(super) fn decode_opcode(
        &self,
        mut body: FunctionBuilder<'_>,
        cursor: RuntimeCursor<'_>,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        let forms = opcode_forms(cursor.opcode_map())
            .filter(|form| !cursor.repeat_prefix() || form.supports_repeat());
        let mut actions: BTreeMap<_, _> = forms_by_opcode(forms)
            .into_iter()
            .map(|(opcode, forms)| (opcode, OpcodeAction::Instruction(forms)))
            .collect();
        if cursor.opcode_map() == OpcodeMap::Primary {
            if !cursor.repeat_prefix() {
                actions.insert(u32::from(EXTENDED_OPCODE_ESCAPE), OpcodeAction::ExtendedMap);
            }
            actions.insert(
                u32::from(OPERAND_SIZE_PREFIX),
                OpcodeAction::OperandSizePrefix,
            );
            actions.insert(u32::from(REPEAT_PREFIX), OpcodeAction::RepeatPrefix);
        }
        let opcodes: Vec<_> = actions.keys().copied().collect();
        body.switch(opcode, &opcodes, |mut arm, key| {
            let Some(opcode_case) = key else {
                return cursor.return_unsupported(arm, opcode);
            };
            match &actions[&opcode_case] {
                OpcodeAction::Instruction(forms) => {
                    let form = forms[0];
                    if form.encoding.has_modrm() {
                        self.decode_modrm_operands(arm, cursor.clone(), opcode, forms)
                    } else {
                        let mut sized = form.with_operand_size(cursor.operand_size());
                        if cursor.repeat_prefix() {
                            sized = sized.with_repeat();
                        }
                        self.decode_opcode_operands(arm, cursor.clone(), opcode_case as u8, &sized)
                    }
                }
                OpcodeAction::ExtendedMap => {
                    let mut extended = cursor.clone();
                    extended.enter_extended_map();
                    let selector = extended.byte(&mut arm)?;
                    self.decode_opcode(arm, extended, &selector)
                }
                OpcodeAction::OperandSizePrefix => {
                    let mut prefixed = cursor.clone();
                    prefixed.select_word_operands();
                    let opcode = prefixed.byte(&mut arm)?;
                    self.opcode_handlers
                        .tail_call(arm, &prefixed, &[(&opcode).into()])
                }
                OpcodeAction::RepeatPrefix => {
                    let mut prefixed = cursor.clone();
                    prefixed.select_repeat();
                    let opcode = prefixed.byte(&mut arm)?;
                    self.opcode_handlers
                        .tail_call(arm, &prefixed, &[(&opcode).into()])
                }
            }
        })?;
        body.trap()
    }
}
