use relocade_compiler::{Program, I32};
use relocade_machine::MachineConfig;
use relocade_runtime::{Cpu, DispatchEntry, RuntimeState};
use relocade_x86::{
    abi, Compiler, CpuState, ExecutionContext, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::mem::offset_of;

use relocade_wasm_runtime::Backend;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_dispatch_table() {
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let backend = Backend::with_state(RuntimeState::new(())).unwrap();
    let mut runtime = Cpu::new(backend, config).await.unwrap();
    // JMP 0200. An uninstalled destination faults; only our callable writes the marker.
    runtime
        .context()
        .memory()
        .write(0x100, &[0xe9, 0xfd, 0])
        .unwrap();
    for address in [0x200, 0x250] {
        runtime
            .context()
            .memory()
            .write(address, &[0x0f, 0x0b])
            .unwrap(); // UD2
    }
    runtime
        .context()
        .memory()
        .write(0x300, &[0x40, 0xe9, 0xfc, 0xfd])
        .unwrap(); // INC AX; JMP 0100
    let module = callable(10);
    runtime
        .install_module(
            &module,
            &[DispatchEntry {
                address: 0x200,
                export: "entry",
            }],
        )
        .await
        .unwrap();
    // A later batch grows both tables while preserving the first callable.
    let mut entries: Vec<_> = (0..4094)
        .map(|offset| DispatchEntry {
            address: 0x10000 + offset,
            export: "entry",
        })
        .collect();
    entries.push(DispatchEntry {
        address: 0x250,
        export: "entry",
    });
    runtime.install_module(&module, &entries).await.unwrap();
    assert_eq!(runtime.run(3).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().ax(), 11);
    assert_eq!(runtime.context().cpu().eip(), 0x100);
    let mut marker = [0; 4];
    runtime.context().memory().read(0x400, &mut marker).unwrap();
    assert_eq!(u32::from_le_bytes(marker), 10);

    let module = callable(20);
    assert!(runtime
        .install_module(
            &module,
            &[
                DispatchEntry {
                    address: 0x200,
                    export: "entry"
                },
                DispatchEntry {
                    address: 0x300,
                    export: "missing"
                },
            ]
        )
        .await
        .is_err());
    // Failure to resolve a later export must not replace the earlier entry.
    assert_eq!(runtime.run(3).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().ax(), 22);
    runtime
        .install_module(
            &module,
            &[DispatchEntry {
                address: 0x200,
                export: "entry",
            }],
        )
        .await
        .unwrap();
    assert_eq!(runtime.run(3).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().ax(), 43);
    assert_eq!(runtime.context().cpu().eip(), 0x100);
    assert_eq!(runtime.context().cpu().instruction_count(), 9);

    // The final slot in the later batch also dispatches to its installed function.
    runtime.context().cpu().set_eip(0x250);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().ax(), 54);
    assert_eq!(runtime.context().cpu().eip(), 0x100);
    assert_eq!(runtime.context().cpu().instruction_count(), 11);
}

fn callable(increment: u32) -> Vec<u8> {
    let mut program = Program::new();
    let context = program.import_memory(abi::EXECUTION_CONTEXT);
    let guest = program.import_memory(abi::GUEST);
    let dispatch = program.import_function(abi::DISPATCH);
    let eax = offset_of!(ExecutionContext, cpu.registers.eax) as u32;
    let eip = offset_of!(ExecutionContext, cpu.eip) as u32;
    let function = program
        .function(abi::RUN.signature(), |mut body| {
            let value = body.load::<I32>(context, eax)?.add(increment);
            body.store(context, eax, value)?;
            body.store::<I32>(guest, 0x400_u32, increment)?;
            body.store::<I32>(context, eip, 0x300_u32)?;
            body.tail_call(dispatch, &[])
        })
        .unwrap();
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
