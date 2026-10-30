use futures_channel::oneshot;
use relocade_machine::{
    Cpu, CpuConfig, ExecutionWorker, Hook, HookResult, IoBus, MachineConfig, Memory, TimerHandle,
};
use relocade_vga::{FrameSource, Vga, PORT_RANGES, TEXT_MEMORY_RANGE};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

// Shared guest workload for the explicitly assembled software and GPU workers.
pub(super) struct Guest {
    pub execution: ExecutionWorker<()>,
    pub source: FrameSource,
    pub progress: Arc<AtomicUsize>,
}

pub(super) async fn spawn(clock: TimerHandle) -> Guest {
    let (send_source, source) = oneshot::channel();
    let progress = Arc::new(AtomicUsize::new(0));
    let cpu_progress = progress.clone();
    let execution = ExecutionWorker::spawn(clock, async move |runner| {
        let vga = Rc::new(RefCell::new(Vga::with_clock(Rc::new(
            runner.clock().clone(),
        ))));
        send_source.send(vga.borrow().frame_source()).ok().unwrap();
        let config = MachineConfig::new(
            0x1000,
            PhysicalMemoryMap::new([
                (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
                (TEXT_MEMORY_RANGE, PhysicalMapping::Mmio),
            ])
            .unwrap(),
        )
        .unwrap();
        let memory = Memory::new(config).unwrap();
        memory
            .view()
            .write(
                0x100,
                &[
                    0xb8, 0x00, 0xb8, 0x8e, 0xc0, // ES = B800.
                    0xb8, 0xdb, 0x1e, 0x26, 0xa3, 0, 0, // First cell: yellow block.
                    0x43, 0x26, 0x89, 0x1e, 2, 0, // INC BX; write changing second cell.
                    0xba, 0xda, 0x03, 0xec, // Read the live VGA status port.
                    0xea, 0, 2, 0, 0, // JMP 0000:0200, progress hook.
                ],
            )
            .unwrap();
        let cpu_config = CpuConfig::new(
            Compiler::new(ExecutionProfile::Real16).target(),
            CpuState {
                eip: 0x100,
                segments: Segments::real_mode(),
                ..CpuState::default()
            },
        )
        .unwrap();
        let io = IoBus::new([runner.connect(vga, [TEXT_MEMORY_RANGE], PORT_RANGES)]);
        let mut cpu = Cpu::new(memory, cpu_config, io, cpu_progress)
            .await
            .unwrap();
        cpu.install_hooks([Hook::<Arc<AtomicUsize>>::new(0x200, 0x201, async |call| {
            call.with(|call| {
                assert_eq!(call.cpu.al() & !9, 0);
                call.state.fetch_add(1, Ordering::Release);
                call.cpu.set_eip(0x100);
                HookResult::Continue
            })
        })])
        .await
        .unwrap();
        runner.run_cpu(&mut cpu).await;
    });

    let source = source.await.unwrap();
    let mut ready = source.next_capture(None).await.unwrap();
    while ready.screen.cells[0].character != 0xdb || progress.load(Ordering::Acquire) == 0 {
        ready = source.next_capture(Some(ready.sequence)).await.unwrap();
    }
    Guest {
        execution,
        source,
        progress,
    }
}
