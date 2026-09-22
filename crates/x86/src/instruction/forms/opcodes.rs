//! Exact opcode cases for shared instruction forms.
use std::collections::BTreeMap;

use super::Form;

/// Forms sharing an opcode become one case; masked patterns contribute every
/// byte they accept. ModRM selects the form within each opcode case.
pub(crate) fn forms_by_opcode<'a>(
    forms: impl Iterator<Item = &'a Form>,
) -> BTreeMap<u32, Vec<&'a Form>> {
    let mut opcodes = BTreeMap::new();
    for form in forms {
        for opcode in 0..=u8::MAX {
            if form.matches(opcode) {
                opcodes
                    .entry(u32::from(opcode))
                    .or_insert_with(Vec::new)
                    .push(form);
            }
        }
    }
    opcodes
}
