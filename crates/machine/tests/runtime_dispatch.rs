mod support;
use relocade_compiler::{Program, I32};

use relocade_machine::DispatchEntry;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler, WasmRuntime};
use relocade_x86::{
    abi, Compiler, CpuState, ExecutionContext, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::mem::offset_of;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_dispatch_table() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut runtime = support::create_cpu(config).await.unwrap();
    // JMP 0200. An uninstalled destination faults; only our callable writes the marker.
    runtime
        .context()
        .memory
        .write(0x100, &[0xe9, 0xfd, 0])
        .unwrap();
    for address in [0x200, 0x250] {
        runtime
            .context()
            .memory
            .write(address, &[0x0f, 0x0b])
            .unwrap(); // UD2
    }
    runtime
        .context()
        .memory
        .write(0x300, &[0x40, 0xe9, 0xfc, 0xfd])
        .unwrap(); // INC AX; JMP 0100
    let module = callable(10);
    let entries = [DispatchEntry {
        key: 0x200,
        export: "entry",
    }];
    // Compilation is independent of the CPU's Wasm runtime. Reuse this module for both batches.
    let module = WasmRuntime::new()
        .unwrap()
        .compiler()
        .compile(&module)
        .await
        .unwrap();
    runtime.install_compiled_module(&module, &entries).unwrap();
    assert_eq!(runtime.context().cpu.eip(), 0x100);
    // A later batch grows both tables while preserving the first callable.
    let mut entries: Vec<_> = (0..4094)
        .map(|offset| DispatchEntry {
            key: 0x10000 + offset,
            export: "entry",
        })
        .collect();
    entries.push(DispatchEntry {
        key: 0x250,
        export: "entry",
    });
    runtime.install_compiled_module(&module, &entries).unwrap();
    assert_eq!(
        support::run_until(&mut runtime, 0x100).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runtime.context().cpu.ax(), 11);
    assert_eq!(runtime.context().cpu.eip(), 0x100);
    let mut marker = [0; 4];
    runtime.context().memory.read(0x400, &mut marker).unwrap();
    assert_eq!(u32::from_le_bytes(marker), 10);

    let module = callable(20);
    let compiled = WasmRuntime::new()
        .unwrap()
        .compiler()
        .compile(&module)
        .await
        .unwrap();
    assert!(runtime
        .install_compiled_module(
            &compiled,
            &[
                DispatchEntry {
                    key: 0x200,
                    export: "entry"
                },
                DispatchEntry {
                    key: 0x300,
                    export: "missing"
                },
            ]
        )
        .is_err());
    // Failure to resolve a later export must not replace the earlier entry.
    assert_eq!(
        support::run_until(&mut runtime, 0x100).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runtime.context().cpu.ax(), 22);
    runtime
        .install_module(
            &module,
            &[DispatchEntry {
                key: 0x200,
                export: "entry",
            }],
        )
        .await
        .unwrap();
    assert_eq!(
        support::run_until(&mut runtime, 0x100).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runtime.context().cpu.ax(), 43);
    assert_eq!(runtime.context().cpu.eip(), 0x100);
    assert_eq!(runtime.context().cpu.instruction_count(), 9);

    // The final slot in the later batch also dispatches to its installed function.
    runtime.context().cpu.set_eip(0x250);
    assert_eq!(
        support::checkpoint(&mut runtime).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runtime.context().cpu.ax(), 54);
    assert_eq!(runtime.context().cpu.eip(), 0x100);
    assert_eq!(runtime.context().cpu.instruction_count(), 11);
}

fn callable(increment: u32) -> Vec<u8> {
    let mut program = Program::new();
    let context = program.import_memory(abi::EXECUTION_CONTEXT);
    let guest = program.import_memory(abi::GUEST);
    let dispatch = program.import_function(abi::DISPATCH);
    let eax = offset_of!(ExecutionContext, cpu.registers.eax) as u32;
    let eip = offset_of!(ExecutionContext, cpu.eip) as u32;
    let function = program.function::<abi::ExecutionSignature>(move |mut body, ()| {
        let value = body.load::<I32>(context, eax)?.add(increment);
        body.store(context, eax, value)?;
        body.store::<I32>(guest, 0x400_u32, increment)?;
        body.store::<I32>(context, eip, 0x300_u32)?;
        body.tail_call(dispatch, &[])
    });
    program.export("entry", function).unwrap();
    program.compile().unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn installed_functions_share_dispatch_and_can_be_replaced() {
    pollster::block_on(check_dispatch_table());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn installed_functions_share_dispatch_and_can_be_replaced() {
    check_dispatch_table().await;
}
