use futures_channel::oneshot;
use futures_util::{
    future::{join, poll_fn},
    FutureExt,
};
use relocade_machine::{
    timer_service, worker::Thread, Cpu, CpuConfig, ExecutionWorker, Hook, HookResult, IoBus,
    MachineConfig, Memory,
};
use relocade_vga::{Vga, VideoFrame, PORT_RANGES, TEXT_MEMORY_RANGE};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{cell::RefCell, future::Future, pin::pin, rc::Rc};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn vga_workers() -> Result<(), String> {
    let (waiting, input_waiting) = oneshot::channel();
    let (source, display_source) = oneshot::channel();
    let (input, incoming) = oneshot::channel();
    let (rendered, presented) = oneshot::channel();
    let (clock, timers) = timer_service();
    let display = Thread::new();
    let cpu_task = ExecutionWorker::spawn(clock, async move |runner| {
        let vga = Rc::new(RefCell::new(Vga::with_clock(Rc::new(
            runner.clock().clone(),
        ))));
        assert!(source.send(vga.borrow().display()).is_ok());
        let config = MachineConfig::new(
            0x1000,
            PhysicalMemoryMap::new([
                (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
                (TEXT_MEMORY_RANGE, PhysicalMapping::Mmio),
            ])
            .unwrap(),
        )
        .unwrap();
        let io = IoBus::new([runner.connect(vga.clone(), [TEXT_MEMORY_RANGE], PORT_RANGES)]);
        let memory = Memory::new(config).unwrap();
        memory
            .view()
            .write(
                0x100,
                &[
                    0xba, 0xda, 0x03, // DX = Input Status 1.
                    0xec, 0xa8, 8, 0x75, 0xfb, // Wait until vertical retrace ends.
                    0xec, 0xa8, 8, 0x74, 0xfb, // Wait for the next vertical retrace.
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
            io,
            Input {
                incoming,
                waiting: Some(waiting),
            },
        )
        .await
        .unwrap();
        cpu.install_hooks([
            Hook::<Input>::new(0x200, async |call| {
                // The local bus and mapped view remain usable across a service await.
                let io = call.io;
                let memory = &mut call.memory;
                io.write_port(0x3d4, 2, 0x060b);
                assert_eq!(memory.read_u16(0xb8020).unwrap(), 0x1e41);
                call.state.waiting.take().unwrap().send(()).unwrap();
                let input = &mut call.state.incoming;
                let value = poll_fn(|context| input.poll_unpin(context)).await.unwrap();
                assert_eq!(io.read_port(0x3d4, 2), 0x060b);
                memory.write_u8(0xb8022, b'C').unwrap();
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
        assert_eq!(
            runner.run_cpu(&mut cpu).await,
            ExecutionExit::Host(HostExit::Return)
        );
        presented.await.unwrap();
        drop(cpu);
        drop(vga);
    });
    let display_task = display.run_send((), move |()| async move {
        let display = display_source.await.unwrap();
        input_waiting.await.unwrap();
        // Read only pixels on this worker; device state remains on execution.
        let mut after = None;
        let frame = loop {
            let frame = display.next_frame(after).await.unwrap();
            if glyph_top(&frame, 0, [255, 255, 85, 255]) == [124, 198, 198, 254, 198]
                && glyph_top(&frame, 1, [255, 255, 255, 255]) == [252, 102, 102, 124, 102]
            {
                break frame;
            }
            after = Some(frame.sequence);
        };
        assert_eq!(frame.pixels.len(), 640 * 200 * 4);
        let retained = frame.pixels.to_vec();
        let mut input = Some(input);
        let mut sequence = frame.sequence;
        let next = loop {
            let mut next = pin!(display.next_frame(Some(sequence)));
            let next = poll_fn(|context| {
                let result = next.as_mut().poll(context);
                if result.is_pending() {
                    if let Some(input) = input.take() {
                        input.send(b'Z').unwrap();
                    }
                }
                result
            })
            .await
            .unwrap();
            sequence = next.sequence;
            if glyph_top(&next, 0, [255, 255, 85, 255]) == [254, 198, 140, 24, 50] {
                break next;
            }
        };
        assert_eq!(
            glyph_top(&next, 1, [255, 255, 255, 255]),
            [124, 198, 192, 192, 192]
        );
        assert_eq!(&*frame.pixels, retained);
        rendered.send(()).unwrap();
        while let Some(frame) = display.next_frame(Some(sequence)).await {
            sequence = frame.sequence;
        }
        assert_eq!(
            glyph_top(&display.current_frame().unwrap(), 0, [255, 255, 85, 255]),
            [254, 198, 140, 24, 50]
        );
    });
    let ((), result) = join(timers.run(), cpu_task.join()).await;
    assert_eq!(result.map_err(|error| error.to_string())?, Some(()));
    display_task.await.map_err(|error| error.to_string())?;
    Ok(())
}

struct Input {
    incoming: oneshot::Receiver<u8>,
    waiting: Option<oneshot::Sender<()>>,
}

// Decode the top five rows for literal glyph assertions. Cursor blinking only
// affects the bottom rows in these fixtures; it must not gate test progress.
pub(crate) fn glyph_top(frame: &VideoFrame, column: usize, foreground: [u8; 4]) -> [u8; 5] {
    std::array::from_fn(|y| {
        (0..8).fold(0, |bits, x| {
            let at = (y * frame.width as usize + column * 8 + x) * 4;
            (bits << 1) | u8::from(frame.pixels[at..at + 4] == foreground)
        })
    })
}
