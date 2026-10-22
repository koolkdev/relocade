use super::{DispatchEntry, DispatchTable, EntryKind};
use relocade_compiler::Program;
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler};
use relocade_x86::abi;

async fn check_entry_lifetimes() {
    let mut backend = Backend::new().unwrap();
    let mut program = Program::new();
    let function = program
        .function(abi::RUN.signature(), |body| body.return_(0_u64))
        .unwrap();
    program.export("entry", function).unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let instance = backend.instantiate(&module, &[]).await.unwrap();
    let mut table = DispatchTable::new(&mut backend).unwrap();
    table.synchronize(&mut backend, 0);
    for (address, kind) in [(0x100, EntryKind::Generated), (0x200, EntryKind::Hook)] {
        table
            .install(
                &mut backend,
                &instance,
                &[DispatchEntry {
                    address,
                    export: "entry",
                }],
                kind,
            )
            .unwrap();
    }
    let generated_slot = table.addresses.get(0x100).unwrap();
    let hook_slot = table.addresses.get(0x200).unwrap();
    table.synchronize(&mut backend, 1);
    assert_eq!(table.addresses.get(0x100), None);
    assert_eq!(table.addresses.get(0x200), Some(hook_slot));
    table
        .install(
            &mut backend,
            &instance,
            &[DispatchEntry {
                address: 0x300,
                export: "entry",
            }],
            EntryKind::Generated,
        )
        .unwrap();
    assert_eq!(table.addresses.get(0x300), Some(generated_slot));
    // Replacements take the new owner's lifetime in both directions.
    for (address, kind) in [(0x200, EntryKind::Generated), (0x300, EntryKind::Hook)] {
        table
            .install(
                &mut backend,
                &instance,
                &[DispatchEntry {
                    address,
                    export: "entry",
                }],
                kind,
            )
            .unwrap();
    }
    for generation in 2..10 {
        table.synchronize(&mut backend, generation);
        assert_eq!(table.addresses.get(0x200), None);
        assert_eq!(table.addresses.get(0x300), Some(generated_slot));
        let address = 0x1000 + generation as u32;
        table
            .install(
                &mut backend,
                &instance,
                &[DispatchEntry {
                    address,
                    export: "entry",
                }],
                EntryKind::Generated,
            )
            .unwrap();
        assert_eq!(table.addresses.get(address), Some(hook_slot));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn entry_replacement_controls_lifetime_and_remaps_reuse_function_slots() {
    pollster::block_on(check_entry_lifetimes());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn entry_replacement_controls_lifetime_and_remaps_reuse_function_slots() {
    check_entry_lifetimes().await;
}
