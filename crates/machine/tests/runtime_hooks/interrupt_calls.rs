use super::{
    support, Compiler, CpuState, ExecutionExit, ExecutionProfile, FarPointer, Interrupts,
    PhysicalMapping, PhysicalMemoryMap, Segments,
};
use relocade_machine::registers as reg;
use std::{cell::Cell, rc::Rc};

async fn check_interrupt_call() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x20000,
        PhysicalMemoryMap::new([
            (0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x10000..=0x10fff,
                PhysicalMapping::Rom {
                    backing_offset: 0x1f000,
                },
            ),
        ])
        .unwrap(),
    )
    .unwrap();
    let mut cpu = support::create_cpu(config).await.unwrap();
    let state = cpu.cpu_state();
    state.set_sp(0xff00);
    state.set_eflags(0x643);
    state.set_eax(0x5678_1234);
    let memory = cpu.memory();
    memory
        .view()
        .write(0x100, &[0xcd, 0x60, 0xeb, 0xfe])
        .unwrap();
    let entry = FarPointer {
        segment: 0x1000,
        offset: 0x20,
    };
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let mut interrupts = Interrupts::new(&mut cpu);
    interrupts.register_interrupt(0x60, entry, async move |call| {
        let frame = relocade_machine::real_mode_interrupt::read_frame(call.cpu, &call.memory);
        assert_eq!(
            frame.return_address,
            FarPointer {
                segment: 0,
                offset: 0x102
            }
        );
        assert_eq!(frame.flags, 0x643);
        assert_eq!(call.cpu.ax(), 0x1234);
        assert_eq!(observed.replace(1), 0);
        assert!(!call.cpu.interrupts_enabled());
        let result = call
            .cpu
            .with_saved_registers([reg::ax], async {
                call.call_interrupt(0x61).await;
                assert_eq!(call.cpu.ax(), 0x1235);
                call.cpu.set_eax(0xabcd_1235);
                42
            })
            .await;
        assert_eq!(result, 42);
        assert_eq!(call.cpu.eax(), 0xabcd_1234);
        assert_eq!(
            call.cpu.sp(),
            0xfefa,
            "callback consumed only its own frame"
        );
        assert_eq!(observed.replace(2), 1);
    });
    interrupts.register_interrupt(0x61, entry.wrapping_add(0x10), async |_| {});
    interrupts.install().await.unwrap();
    // Replace the callback after registration. Its IRET resumes the Rust await;
    // the host interrupt's own IRET returns to the original caller.
    memory.view().write_u32(0x61 * 4, 0x500).unwrap();
    memory.view().write(0x500, &[0x40, 0xeb, 0, 0xcf]).unwrap(); // INC AX; JMP 0503; IRET.
    assert_eq!(
        support::run_until(&mut cpu, 0x503).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(calls.get(), 1, "a checkpoint retains the waiting caller");
    assert_eq!(
        support::run_until(&mut cpu, 0x102).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(state.eax(), 0xabcd_1234);
    assert_eq!(state.sp(), 0xff00);
    assert_eq!(state.eflags(), 0x643);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_handlers_await_the_live_interrupt_vector() {
    pollster::block_on(check_interrupt_call());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_handlers_await_the_live_interrupt_vector() {
    check_interrupt_call().await;
}
