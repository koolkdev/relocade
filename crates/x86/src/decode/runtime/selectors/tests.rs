//! Fixed-byte forms must not duplicate neighboring register-range handlers.

use std::cell::RefCell;

use wasm86_compiler::{Program, Signature, Type};
use wasmparser::Validator;

use super::*;

#[test]
fn mixed_modrm_selectors_lower_each_form_once() {
    let state = DecodeState::default();
    for opcode in [0xd9, 0xdb, 0xdd] {
        let forms: Vec<_> = state.forms().filter(|form| form.matches(opcode)).collect();
        let calls = RefCell::new(vec![0; forms.len()]);
        let mut program = Program::new();
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I32],
                },
                |body| {
                    let modrm = body.parameter::<I32>(0)?.truncate::<I8>();
                    dispatch_modrm_form(body, &modrm, &forms, &state, &|arm, form| {
                        let index = form.map(|form| {
                            let index = forms
                                .iter()
                                .position(|candidate| std::ptr::eq(*candidate, form))
                                .unwrap();
                            calls.borrow_mut()[index] += 1;
                            index as u32
                        });
                        arm.return_(index.unwrap_or(u32::MAX))
                    })
                },
            )
            .unwrap();
        program.export("select", function).unwrap();
        Validator::new()
            .validate_all(&program.compile().unwrap())
            .unwrap();
        assert_eq!(*calls.borrow(), vec![1; forms.len()], "opcode {opcode:02x}");
    }
}
