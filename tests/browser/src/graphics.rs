mod target;

use futures_channel::{mpsc, oneshot};
use futures_util::{future::join, FutureExt, StreamExt};
use relocade_machine::{
    timer_service, worker::Thread, Cpu, CpuConfig, ExecutionWorker, Hook, HookResult, IoBus,
    MachineConfig, Memory,
};
use relocade_vga::{Vga, VideoFrame, PORT_RANGES, TEXT_MEMORY_RANGE};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};
use target::Target;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn graphics_workers() -> Result<(), String> {
    let (clock, timers) = timer_service();
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
        cpu.install_hooks([Hook::<Arc<AtomicUsize>>::new(0x200, async |call| {
            assert_eq!(call.cpu.al() & !9, 0);
            call.state.fetch_add(1, Ordering::Release);
            call.cpu.set_eip(0x100);
            HookResult::Continue
        })])
        .await
        .unwrap();
        runner.run_cpu(&mut cpu).await;
    });

    let app = async move {
        let source = source.await.unwrap();
        // A single Sender with buffer size zero has one reserved request slot.
        // This fixture app drops excess requests; VGA owns no request protocol.
        let (mut requests, mut incoming) = mpsc::channel::<[u32; 2]>(0);
        let (selected, selected_capture) = oneshot::channel();
        let (convert, conversion_allowed) = oneshot::channel();
        let (resume, resumed) = oneshot::channel();
        let (report, mut reports) = mpsc::unbounded();
        let conversions = Arc::new(AtomicUsize::new(0));
        let conversion_count = conversions.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let target_drop = dropped.clone();
        let worker_source = source.clone();
        let graphics = Thread::new();
        let task = graphics.run_send(target::context()?, move |context| async move {
            let mut target = Target::new(context, target_drop).await?;
            let mut cached: Option<Arc<VideoFrame>> = None;
            let mut before_conversion = Some((selected, conversion_allowed));
            let mut after_present = Some(resumed);
            while let Some(size) = incoming.next().await {
                let Some(capture) = worker_source.next_capture(None).await else {
                    continue;
                };
                if let Some((selected, allowed)) = before_conversion.take() {
                    selected.send(capture.sequence).unwrap();
                    allowed.await.unwrap(); // Hold the first conversion, not the CPU.
                }
                if cached
                    .as_ref()
                    .is_none_or(|image| image.sequence != capture.sequence)
                {
                    conversion_count.fetch_add(1, Ordering::Release);
                    cached = Some(Arc::new(capture.rasterize()));
                }
                let image = cached.as_ref().unwrap().clone();
                let presented = target.present(&image, size).await?;
                report.unbounded_send((presented, image)).unwrap();
                if let Some(resumed) = after_present.take() {
                    resumed.await.unwrap();
                }
            }
            Ok::<(), String>(())
        });

        let mut ready = source.next_capture(None).await.unwrap();
        while ready.screen.cells[0].character != 0xdb || progress.load(Ordering::Acquire) == 0 {
            ready = source.next_capture(Some(ready.sequence)).await.unwrap();
        }
        request(&mut requests, [64, 48]);
        let selected = selected_capture.await.unwrap();
        let mut sequence = source.next_capture(Some(selected)).await.unwrap().sequence;
        while progress.load(Ordering::Acquire) == 0 {
            sequence = source.next_capture(Some(sequence)).await.unwrap().sequence;
        }
        let before = progress.load(Ordering::Acquire);
        for _ in 0..10 {
            for _ in 0..100 {
                request(&mut requests, [96, 72]);
            }
            sequence = source.next_capture(Some(sequence)).await.unwrap().sequence;
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(conversions.load(Ordering::Acquire), 0);
        convert.send(()).unwrap();
        let (first, first_image) = reports.next().await.unwrap();
        assert_eq!(first.sequence, selected);
        assert_eq!(first.size, [64, 48]);
        assert_eq!(first.pixel, [255, 255, 85, 255]);

        // Holding presentation also leaves only one pending request. The next
        // conversion selects a recent capture rather than replaying a backlog.
        let before = progress.load(Ordering::Acquire);
        for _ in 0..10 {
            for _ in 0..100 {
                request(&mut requests, [96, 72]);
            }
            sequence = source.next_capture(Some(sequence)).await.unwrap().sequence;
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(conversions.load(Ordering::Acquire), 1);
        resume.send(()).unwrap();
        let (second, retained) = reports.next().await.unwrap();
        assert!(second.sequence >= sequence);
        assert_eq!(second.size, [96, 72]);
        assert_eq!(second.count, 2);
        assert_eq!(second.pixel, first.pixel);
        assert!(!Arc::ptr_eq(&first_image, &retained));

        // No requests: captures and the CPU progress without any conversion.
        let before = progress.load(Ordering::Acquire);
        sequence = second.sequence;
        for _ in 0..10 {
            sequence = source.next_capture(Some(sequence)).await.unwrap().sequence;
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(conversions.load(Ordering::Acquire), 2);
        assert!(reports.next().now_or_never().is_none());
        request(&mut requests, [48, 36]);
        let (third, _) = reports.next().await.unwrap();
        assert!(third.sequence >= sequence);
        assert_eq!(third.size, [48, 36]);
        assert_eq!(third.count, 3);
        assert_eq!(third.pixel, first.pixel);

        // Source closure does not close the host's surface or its worker. The
        // application may render the last capture and present it repeatedly.
        assert_eq!(execution.stop().await.map_err(|e| e.to_string())?, None);
        let last = source.current_capture().unwrap();
        assert!(source.next_capture(Some(last.sequence)).await.is_none());
        assert!(!dropped.load(Ordering::Acquire));
        request(&mut requests, [64, 48]);
        let (closed, last_image) = reports.next().await.unwrap();
        assert_eq!(closed.sequence, last.sequence);
        let count = conversions.load(Ordering::Acquire);
        request(&mut requests, [64, 48]);
        let (repeated, repeated_image) = reports.next().await.unwrap();
        assert_eq!(repeated.count, closed.count + 1);
        assert!(Arc::ptr_eq(&last_image, &repeated_image));
        assert_eq!(conversions.load(Ordering::Acquire), count);
        assert!(!dropped.load(Ordering::Acquire));
        drop(requests); // Only the host decides when presentation ends.
        task.await.map_err(|e| e.to_string())??;
        assert!(dropped.load(Ordering::Acquire));
        Ok(())
    };
    let ((), result) = join(timers.run(), app).await;
    result
}

// Coalescing is a fixture policy. There is one sender, hence one pending slot;
// a full channel already represents the requested redraw opportunity.
pub(crate) fn request(requests: &mut mpsc::Sender<[u32; 2]>, size: [u32; 2]) {
    if let Err(error) = requests.try_send(size) {
        assert!(error.is_full(), "graphics worker must still be listening");
    }
}
