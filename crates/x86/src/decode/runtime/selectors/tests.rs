//! Runtime selector dispatch agrees with forms and lowers each handler once.

use std::cell::RefCell;

use wasm86_compiler::{Program, Signature, Type};
use wasm86_test_support::{engine, Module};
use wasmparser::Validator;

use super::*;

#[test]
fn mixed_modrm_selectors_lower_each_form_once() {
    let state = DecodeState::default();
    // Ordinary /r, /n and memory-only forms also retain their selection policy.
    for opcode in [0x01, 0x80, 0x8d, 0xd9, 0xdb, 0xdd] {
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
        let bytes = program.compile().unwrap();
        Validator::new().validate_all(&bytes).unwrap();
        assert_eq!(*calls.borrow(), vec![1; forms.len()], "opcode {opcode:02x}");

        let module = Module::new(&bytes);
        let mut store = wasmtime::Store::new(engine(), ());
        let instance = wasmtime::Instance::new(&mut store, module.wasmtime(), &[]).unwrap();
        let select = instance
            .get_typed_func::<u32, u32>(&mut store, "select")
            .unwrap();
        for modrm in 0..=u8::MAX {
            let index = select.call(&mut store, u32::from(modrm)).unwrap();
            // The selected form's addressing policy is enforced by the operand
            // decoder. Fixed selector bits must already match at this boundary.
            let actual = forms
                .get(index as usize)
                .filter(|form| {
                    if modrm >> 6 == 3 {
                        form.accepts_register_rm(&state.prefixes)
                    } else {
                        form.accepts_memory_rm()
                    }
                })
                .map(|_| index as usize);
            let expected = forms
                .iter()
                .position(|form| form.matches_modrm(modrm, &state.prefixes));
            assert_eq!(actual, expected, "opcode {opcode:02x}, ModRM {modrm:02x}");
        }
    }
}
