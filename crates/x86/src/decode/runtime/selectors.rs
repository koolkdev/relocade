use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::instruction::{
    is_set_condition, DecodedInstruction, ACCUMULATOR_IMMEDIATE_FORMS, ARITHMETIC_MODRM_FORMS,
    EXTENDED_OPCODE_ESCAPE, OPERAND_SIZE_PREFIX, SET_CONDITION_FORMS,
};

use super::{cursor::RuntimeCursor, RuntimeDecoder};

impl<C> RuntimeDecoder<C>
where
    C: Fn(FunctionBuilder<'_>, DecodedInstruction<Val<I32>, Val<I32>>) -> Result<(), BuildError>,
{
    pub(super) fn decode_arithmetic_or_escape(
        &self,
        mut body: FunctionBuilder<'_>,
        mut cursor: RuntimeCursor,
        opcode: &Val<I8>,
    ) -> Result<(), BuildError> {
        for form in &ACCUMULATOR_IMMEDIATE_FORMS {
            body.if_(form.matches_value(opcode), |form_body| {
                self.decode_immediate(
                    form_body,
                    cursor.clone(),
                    opcode,
                    &form.resolve(cursor.operand_size()),
                )
            })?;
        }
        // Grouped immediate forms share an opcode. The operand handler reads
        // ModRM once and selects its extension before any address or immediate.
        let arithmetic_opcode = ARITHMETIC_MODRM_FORMS
            .iter()
            .map(|form| form.matches_value(opcode))
            .reduce(|left, right| left.or(right))
            .expect("arithmetic forms exist");
        body.if_(arithmetic_opcode, |form_body| {
            self.arithmetic_handlers
                .tail_call(form_body, &cursor, opcode)
        })?;
        body.if_(
            opcode.eq(u32::from(EXTENDED_OPCODE_ESCAPE)),
            |mut extended_body| {
                let mut extended_cursor = cursor.clone();
                extended_cursor.enter_extended_map();
                let selector = extended_cursor.byte(&mut extended_body)?;
                extended_body.if_(is_set_condition(&selector).eq(0), |unsupported_body| {
                    extended_cursor.return_unsupported(unsupported_body, &selector)
                })?;
                self.decode_modrm(
                    extended_body,
                    extended_cursor,
                    &selector,
                    SET_CONDITION_FORMS.iter(),
                )
            },
        )?;
        body.if_(
            opcode.ne(u32::from(OPERAND_SIZE_PREFIX)),
            |unsupported_body| cursor.return_unsupported(unsupported_body, opcode),
        )?;
        cursor.select_word_operands();
        let opcode = cursor.byte(&mut body)?;
        self.decode_opcode(body, cursor, &opcode)
    }
}
