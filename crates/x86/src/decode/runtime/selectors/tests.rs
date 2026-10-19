//! Runtime selector dispatch agrees with forms and lowers each handler once.

use std::cell::RefCell;

use relocade_compiler::{Program, Signature, Type};
use relocade_test_support::{engine, Module};
use wasmparser::Validator;

use super::*;

#[test]
fn mixed_modrm_selectors_lower_each_form_once() {
    let state = DecodeState::default();
    // Ordinary /r, /n and memory-only forms also retain their selection policy.
    for opcode in [0x01, 0x80, 0x8d, 0xd9, 0xdb, 0xdd] {
        let forms: Vec<_> = state.forms().filter(|form| form.matches(opcode)).collect();
        check_dispatch(&state, opcode, forms);
    }
}

#[test]
fn register_only_forms_reject_memory_without_a_companion_form() {
    let state = DecodeState::default().extended().unwrap();
    for opcode in [0x12, 0x16] {
        let forms: Vec<_> = state.forms().filter(|form| form.matches(opcode)).collect();
        check_dispatch(&state, opcode, forms.clone());
        let register_forms: Vec<_> = forms
            .into_iter()
            .filter(|form| !form.accepts_memory_rm())
            .collect();
        assert_eq!(register_forms.len(), 1);
        check_dispatch(&state, opcode, register_forms);
    }
}

fn check_dispatch(state: &DecodeState, opcode: u8, forms: Vec<&'static Form>) {
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
                dispatch_modrm_form(body, &modrm, &forms, state, &|arm, form| {
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
        // Operand decoding rejects register encodings of memory-only forms.
        // Register-only selection must reject memory encodings itself.
        let actual = forms
            .get(index as usize)
            .filter(|form| modrm >> 6 != 3 || form.accepts_register_rm(&state.prefixes))
            .map(|_| index as usize);
        let expected = forms
            .iter()
            .position(|form| form.matches_modrm(modrm, &state.prefixes));
        assert_eq!(actual, expected, "opcode {opcode:02x}, ModRM {modrm:02x}");
    }
}
