use super::*;
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap, Segments};
use wasmtime::Caller;

fn machine(code: &[u8]) -> Machine<usize> {
    let engine = wasm86_test_support::engine();
    let mut store = Store::new(engine, HostState::new(0));
    let cpu = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    let guest = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    let table = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
    cpu.write(
        &mut store,
        0,
        &CpuState {
            eip: 0x1000,
            segments: Segments::real_mode(),
            ..CpuState::default()
        }
        .to_bytes(),
    )
    .unwrap();
    guest.write(&mut store, 0x1000, code).unwrap();
    let map = PhysicalMemoryMap::new([
        (
            0x1000..=0x1fff,
            PhysicalMapping::Rom {
                backing_offset: 0x1000,
            },
        ),
        (0x2000..=0x2fff, PhysicalMapping::Mmio),
        (
            0x3000..=0x3fff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
    ])
    .unwrap();
    table.write(&mut store, 0, &map.to_bytes()).unwrap();
    let memory = HostMemory::new(&mut store, cpu, guest, table, Profile::Real16);
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            "wasm86",
            "readMmio",
            move |mut caller: Caller<'_, HostState<usize>>, address: u32, bytes: u32| {
                assert_eq!((address, bytes), (0x2000, 1));
                caller.data_mut().host += 1;
                memory.write_backing(&mut caller, 0x1003, &[0x48]).unwrap();
                // Remapping is also available through a Caller. The current read has
                // already reached the device; later accesses use the new RAM routing.
                memory.remap(
                    &mut caller,
                    2,
                    Mapping::Ram {
                        backing: 0x2000,
                        writable: true,
                    },
                );
                0i64
            },
        )
        .unwrap();
    linker
        .func_wrap::<_, ()>("wasm86", "writeMmio", |_: i32, _: i32, _: i64| {
            panic!("unexpected MMIO write")
        })
        .unwrap();
    linker
        .func_wrap("wasm86", "readPort", |_: i32, _: i32| -> i32 {
            panic!("unexpected port read")
        })
        .unwrap();
    linker
        .func_wrap::<_, ()>("wasm86", "writePort", |_: i32, _: i32, _: i32| {
            panic!("unexpected port write")
        })
        .unwrap();
    let mut machine = Machine {
        runtime: Runtime::new(store, linker, memory).unwrap(),
    };
    assert!(matches!(
        machine.wait_for(0, false),
        CompilationEvent::Installed { .. }
    ));
    machine
}

#[test]
fn device_callback_invalidates_upcoming_code_before_interpreter_fetch() {
    // MOV AL, [0x2000]; INC AX. The read callback replaces INC with DEC.
    let mut machine = machine(&[0xa0, 0, 0x20, 0x40]);
    let id = machine.runtime.request_block(0x1000, 2).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Installed { .. }
    ));
    assert_eq!(
        machine.runtime.run_slice(2).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, 0xffff);
    assert_eq!(machine.cpu().instruction_count, 2);
    assert_eq!(machine.runtime.store().data().host, 1);
    assert!(machine.runtime.blocks.keys().all(|ticket| !machine
        .runtime
        .memory()
        .contains(machine.runtime.store(), *ticket)));
}

#[test]
fn ram_alias_write_invalidates_code_fetched_from_rom() {
    // MOV byte [0x3005], 0x48; INC AX. RAM and ROM refer to the same backing.
    let mut machine = machine(&[0xc6, 0x06, 0x05, 0x30, 0x48, 0x40]);
    let id = machine.runtime.request_block(0x1000, 2).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Installed { .. }
    ));
    assert_eq!(
        machine.runtime.run_slice(2).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, 0xffff);
    assert_eq!(machine.cpu().instruction_count, 2);
    assert_eq!(machine.runtime.store().data().host, 0);
}
