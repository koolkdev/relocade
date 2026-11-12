use futures_channel::oneshot;
use relocade_machine::{CpuConfig, ExecutionWorker, Hook, MachineConfig, TimerHandle};
use relocade_vga::{DispiAdapter, DisplaySnapshot, MEMORY_RANGE};
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

// Shared guest workload for the explicitly assembled software and GPU workers.
pub(super) struct Guest {
    pub machine: relocade_machine::Machine,
    pub execution: ExecutionWorker<()>,
    pub source: async_channel::Receiver<DisplaySnapshot>,
    pub retraces: async_channel::Receiver<()>,
    pub progress: Arc<AtomicUsize>,
}

pub(super) async fn spawn(clock: TimerHandle) -> Guest {
    let (send_source, source) = oneshot::channel();
    let (retrace, retraces) = async_channel::bounded(1);
    let progress = Arc::new(AtomicUsize::new(0));
    let cpu_progress = progress.clone();
    let config = MachineConfig::new(
        0x1000,
        PhysicalMemoryMap::new([
            (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
            (MEMORY_RANGE, PhysicalMapping::Mmio),
        ])
        .unwrap(),
    )
    .unwrap();
    let mut machine = relocade_machine::Machine::new(config).unwrap();
    let cpu_config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
    )
    .unwrap();
    let prepared = machine.create_cpu(cpu_config, None).unwrap();
    let execution = prepared.spawn(clock, async move |mut runner| {
        let vga = Rc::new(RefCell::new(DispiAdapter::with_clock(
            runner.clock().clone(),
        )));
        send_source.send(vga.borrow().frames()).ok().unwrap();

        let memory = runner.memory();
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

        runner
            .connect(vga.clone(), |io| DispiAdapter::configure_io(io, &vga))
            .await;
        crate::vga_fixture::initialize_font(&runner.io());
        let cpu = runner.cpu();
        cpu.install([Hook::host(0x200, {
            let cpu = cpu.cpu_state();
            let in_retrace = Cell::new(false);
            async move || {
                assert_eq!(cpu.al() & !9, 0);
                cpu_progress.fetch_add(1, Ordering::Release);
                let active = cpu.al() & 8 != 0;
                let previous = in_retrace.replace(active);
                if active && !previous {
                    retrace.force_send(()).unwrap();
                }
                cpu.set_eip(0x100);
            }
        })])
        .await
        .unwrap();
        runner.run_cpu().await;
    });

    let source = source.await.unwrap();
    Guest {
        machine,
        execution,
        source,
        retraces,
        progress,
    }
}

// Observe guest status reads without consuming the graphics worker's snapshots.
pub(super) async fn wait_retraces(retraces: &async_channel::Receiver<()>, count: usize) {
    while retraces.try_recv().is_ok() {}
    for _ in 0..count {
        retraces.recv().await.unwrap();
    }
}
