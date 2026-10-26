use futures_channel::oneshot;
use futures_util::{future::poll_fn, FutureExt};
use relocade_machine::{
    worker::Thread, Cpu, CpuConfig, DeviceConnection, Hook, HookResult, IoBus, MachineConfig,
    Memory,
};
use relocade_vga::{TextCell, TextRenderer, Vga, CRTC_PORT_RANGE, TEXT_MEMORY_RANGE};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{sync::Arc, time::Duration};

struct Input {
    incoming: oneshot::Receiver<u8>,
    waiting: Option<oneshot::Sender<()>>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn vga_workers() -> Result<(), String> {
    let vga = Arc::new(Vga::new());
    let cpu_vga = vga.clone();
    let (input, incoming) = oneshot::channel();
    let (waiting, started) = oneshot::channel();
    let (finished, exited) = oneshot::channel();
    let execution = Thread::new();
    let display = Thread::new();
    let cpu_task = execution.run_send((), move |()| async move {
        let config = MachineConfig::new(
            0x1000,
            PhysicalMemoryMap::new([
                (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
                (TEXT_MEMORY_RANGE, PhysicalMapping::Mmio),
            ])
            .unwrap(),
        )
        .unwrap();
        let io = IoBus::new([DeviceConnection {
            device: cpu_vga,
            mmio: vec![TEXT_MEMORY_RANGE],
            ports: vec![CRTC_PORT_RANGE],
        }]);
        let memory = Memory::new(config, io).unwrap();
        memory
            .view()
            .write(
                0x100,
                &[
                    0xb8, 0x00, 0xb8, 0x8e, 0xc0, // ES = B800.
                    0x26, 0x66, 0xc7, 0x06, 0x20, 0, b'A', 0x1e, b'B', 0x2f,
                    // MOV dword ES:[0020],2F421E41 writes two cells together.
                    0xba, 0xd4, 0x03, // DX = CRTC index port.
                    0xb8, 0x0d, 0x10, 0xef, // Display starts at cell 0010.
                    0xb8, 0x0f, 0x11, 0xef, // Cursor at cell 0011.
                    0xb8, 0x0a, 0x20, 0xef, // Hide cursor.
                    0xea, 0x00, 0x02, 0, 0, // JMP 0000:0200, the input hook.
                ],
            )
            .unwrap();
        memory
            .view()
            .write(
                0x300,
                &[
                    0x26, 0xa2, 0x20, 0, // MOV ES:[0020],AL after input arrives.
                    0xea, 0x00, 0x04, 0, 0, // JMP 0000:0400, the exit hook.
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
        let mut cpu = Cpu::new(
            memory,
            cpu_config,
            Input {
                incoming,
                waiting: Some(waiting),
            },
        )
        .await
        .unwrap();
        cpu.install_hooks([
            Hook::<Input>::new(0x200, async |call| {
                let input = &mut *call.state;
                let value = poll_fn(|context| {
                    let poll = input.incoming.poll_unpin(context);
                    if poll.is_pending() {
                        if let Some(waiting) = input.waiting.take() {
                            waiting.send(()).unwrap();
                        }
                    }
                    poll
                })
                .await
                .unwrap();
                call.cpu.set_al(value);
                call.cpu.set_eip(0x300);
                HookResult::Continue
            }),
            Hook::<Input>::new(0x400, async |_call| {
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }),
        ])
        .await
        .unwrap();
        assert_eq!(cpu.run(100).await, ExecutionExit::Host(HostExit::Return));
        drop(cpu);
        finished.send(()).unwrap();
    });
    let display_task = display.run_send((), move |()| async move {
        // No input is sent until a snapshot and a frame complete on this worker.
        started.await.unwrap();
        let before = vga.screen();
        assert_eq!(before.start_address, 0x10);
        assert_eq!(before.cursor.address, 0x11);
        assert_eq!(before.cursor.start_scan_line, 0x20);
        assert_eq!(
            before.cells[..2],
            [
                TextCell {
                    character: b'A',
                    attribute: 0x1e
                },
                TextCell {
                    character: b'B',
                    attribute: 0x2f
                },
            ]
        );
        let mut text = TextRenderer::new();
        let pixels = text.render(&before, Duration::ZERO);
        assert_eq!(pixels.len(), 640 * 200 * 4);
        input.send(b'Z').unwrap();
        exited.await.unwrap();
        let after = vga.screen();
        assert_eq!(
            after.cells[0],
            TextCell {
                character: b'Z',
                attribute: 0x1e
            }
        );
        assert_eq!(after.cells[1], before.cells[1]);
        assert_eq!(before.cells[0].character, b'A');
    });
    cpu_task.await.map_err(|error| error.to_string())?;
    display_task.await.map_err(|error| error.to_string())?;
    Ok(())
}
