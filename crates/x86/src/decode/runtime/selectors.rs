//! Prefix transitions, opcode-map escapes and selection of viable forms.
use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    decode::DecodeState,
    instruction::{
        forms_by_opcode, DecodedInstruction, Form, OpcodeMap, Prefix, EXTENDED_OPCODE_ESCAPE,
    },
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

enum OpcodeAction {
    Instruction(Vec<&'static Form>),
    ExtendedMap(DecodeState),
    Prefix(Prefix),
}

impl<C> RuntimeDecoder<'_, C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(super) fn decode_opcode(
        &self,
        mut body: FunctionBuilder<'_>,
        cursor: RuntimeCursor<'_>,
        state: DecodeState,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        let mut actions: BTreeMap<_, _> = forms_by_opcode(state.forms())
            .into_iter()
            .map(|(opcode, forms)| (opcode, OpcodeAction::Instruction(forms)))
            .collect();
        if state.map == OpcodeMap::Primary {
            if let Some(extended) = state.extended() {
                actions.insert(
                    u32::from(EXTENDED_OPCODE_ESCAPE),
                    OpcodeAction::ExtendedMap(extended),
                );
            }
            for prefix in Prefix::ALL {
                actions.insert(u32::from(prefix.byte()), OpcodeAction::Prefix(prefix));
            }
        }
        let opcodes: Vec<_> = actions.keys().copied().collect();
        body.switch(opcode, &opcodes, |mut arm, key| {
            let Some(opcode_case) = key else {
                return state.return_unsupported(arm, &cursor, opcode);
            };
            match &actions[&opcode_case] {
                OpcodeAction::Instruction(forms) => {
                    let form = forms[0];
                    if form.encoding.has_modrm() {
                        self.decode_modrm_operands(
                            arm,
                            cursor.clone(),
                            state.clone(),
                            opcode_case as u8,
                            forms,
                        )
                    } else {
                        let form = form
                            .resolve(&state.prefixes)
                            .expect("opcode selection accepted the prefix state");
                        self.decode_opcode_operands(arm, cursor.clone(), opcode_case as u8, &form)
                    }
                }
                OpcodeAction::ExtendedMap(extended) => {
                    let mut cursor = cursor.clone();
                    let selector = cursor.byte(&mut arm)?;
                    self.decode_opcode(arm, cursor, extended.clone(), &selector)
                }
                OpcodeAction::Prefix(prefix) => {
                    let mut cursor = cursor.clone();
                    let opcode = cursor.byte(&mut arm)?;
                    self.opcode_handlers.tail_call(
                        arm,
                        &cursor,
                        state.with_prefix(*prefix),
                        &[(&opcode).into()],
                    )
                }
            }
        })?;
        body.trap()
    }
}
