//! Selects instruction forms from prefixes, opcode maps and fixed ModRM bits.
use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    decode::DecodeState,
    instruction::{
        forms_by_opcode, DecodedInstruction, Form, OpcodeMap, Prefix, EXTENDED_OPCODE_ESCAPE,
    },
};

use super::{cursor::RuntimeCursor, InstructionDecoder};

#[cfg(test)]
mod tests;

enum OpcodeAction {
    Instruction(Vec<&'static Form>),
    ExtendedMap(DecodeState),
    Prefix(Prefix),
}

impl<C> InstructionDecoder<'_, '_, C>
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
                    self.decoder.opcode_handlers.tail_call(
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

/// Fixed selector bits distinguish forms before address decoding. The selected
/// form then enforces its memory/register policy. Ordinary /r forms need no
/// switch; /n groups continue to select only their three extension bits.
pub(super) fn dispatch_modrm_form(
    body: FunctionBuilder<'_>,
    modrm: &Val<I8>,
    forms: &[&'static Form],
    state: &DecodeState,
    continue_decoding: &impl Fn(FunctionBuilder<'_>, Option<&Form>) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    dispatch_modrm_bits(body, modrm, forms, state, 0, continue_decoding)
}

/// Select shared fixed bits first so an eight-register range lowers one body,
/// even when another form at this opcode selects a complete ModRM byte.
fn dispatch_modrm_bits(
    mut body: FunctionBuilder<'_>,
    modrm: &Val<I8>,
    forms: &[&Form],
    state: &DecodeState,
    tested_mask: u8,
    continue_decoding: &impl Fn(FunctionBuilder<'_>, Option<&Form>) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    if let [form] = forms {
        let selector = form.modrm.expect("a ModRM form has a selector");
        let remaining = selector.mask & !tested_mask;
        if remaining != 0 {
            body.if_(
                modrm
                    .and(u32::from(remaining))
                    .ne(u32::from(selector.fixed_bits() & remaining)),
                |arm| continue_decoding(arm, None),
            )?;
        }
        return continue_decoding(body, Some(form));
    }
    let common = forms.iter().fold(u8::MAX, |mask, form| {
        mask & form.modrm.expect("a ModRM form has a selector").mask
    });
    let mask = if common & !tested_mask != 0 {
        common & !tested_mask
    } else {
        forms.iter().fold(0, |mask, form| {
            mask | form.modrm.expect("a ModRM form has a selector").mask
        }) & !tested_mask
    };
    assert_ne!(mask, 0, "fixed selector bits must distinguish ModRM forms");
    let shift = mask.trailing_zeros();
    let mut choices = BTreeMap::<u32, Vec<&Form>>::new();
    for byte in 0..=u8::MAX {
        for form in forms {
            if form.matches_modrm(byte, &state.prefixes) {
                let key = u32::from(byte & mask) >> shift;
                let candidates = choices.entry(key).or_default();
                if !candidates
                    .iter()
                    .any(|candidate| std::ptr::eq(*candidate, *form))
                {
                    candidates.push(form);
                }
            }
        }
    }
    let keys: Vec<_> = choices.keys().copied().collect();
    body.switch(
        modrm.unsigned().shr(shift).and(u32::from(mask) >> shift),
        &keys,
        |arm, key| match key.and_then(|key| choices.get(&key)) {
            Some(candidates) => dispatch_modrm_bits(
                arm,
                modrm,
                candidates,
                state,
                tested_mask | mask,
                continue_decoding,
            ),
            None => continue_decoding(arm, None),
        },
    )?;
    body.trap()
}
