use crate::software::{self, SoftwareFrame};
use crate::{
    graphics::{request, target},
    vga::glyph_top,
};
use futures_channel::{mpsc, oneshot};
use futures_util::{future::join, FutureExt, StreamExt};
use relocade_dos::Dos;
use relocade_machine::{timer_service, worker::Thread, ExecutionWorker};
use relocade_vga::FrameSource;
use std::{
    future::pending,
    io::Write,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

// Write while busy, print P, await an echoed key, write Z, and exit with code 7.
const FIRST_PROGRAM: &[u8] = &[
    0xb8, 0x00, 0xb8, 0x8e, 0xc0, // ES = B800.
    0x26, 0xc7, 0x06, 0, 0, b'A', 0x1e, // Cell 0 = A.
    0x26, 0xff, 0x06, 2, 0, // Loop: INC word ES:[2].
    0x80, 0x3e, 0, 4, 0, // CMP byte DS:[0400],0.
    0x74, 0xf4, // JE loop.
    0xb4, 2, 0xb2, b'P', 0xcd, 0x21, // DOS print P.
    0xb4, 1, 0xcd, 0x21, // DOS read and echo.
    0x26, 0xc7, 0x06, 4, 0, b'Z', 0x1e, // Last write before exit.
    0xb8, 7, 0x4c, 0xcd, 0x21,
];
const SECOND_PROGRAM: &[u8] = &[
    0xb8, 0x00, 0xb8, 0x8e, 0xc0, 0x26, 0xc7, 0x06, 0, 0, b'S', 0x2f, 0xb8, 9, 0x4c, 0xcd, 0x21,
];

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn dos_workers() -> Result<(), String> {
    let (video, video_ready) = oneshot::channel();
    let (leave_busy, busy_observed) = oneshot::channel();
    let (key, key_input) = oneshot::channel();
    let (first_done, first_exit) = oneshot::channel();
    let (launch_next, next_launch) = oneshot::channel();
    let (second_done, second_exit) = oneshot::channel();
    let (clock, timers) = timer_service();
    let execution = ExecutionWorker::spawn(clock, async move |runner| {
        let mut dos = Dos::new(runner).await.unwrap();
        video.send(dos.frame_source()).ok().unwrap();
        dos.launch_com(FIRST_PROGRAM, Default::default()).unwrap();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        let memory = dos.cpu().memory();
        let mut input = dos.stdin();
        let fixture_input = async {
            busy_observed.await.unwrap();
            memory.view().write_u8(base + 0x400, 1).unwrap();
            // Bridge the app's Rust message to the execution-local DOS stream.
            input.write_all(&[key_input.await.unwrap()]).unwrap();
        };
        let (result, ()) = join(dos.run(runner), fixture_input).await;
        assert_eq!(dos.vga().borrow().snapshot().plane(0)[2], b'X');
        first_done.send(result.unwrap()).unwrap();
        next_launch.await.unwrap();
        dos.launch_com(SECOND_PROGRAM, Default::default()).unwrap();
        second_done.send(dos.run(runner).await.unwrap()).unwrap();
        // Registered VGA stays active between launches and after process exit.
        pending::<()>().await;
    });

    let app = async move {
        let source = video_ready.await.unwrap();
        let (requests, mut incoming) = mpsc::channel::<[u32; 2]>(0);
        let (report, presentations) = mpsc::unbounded();
        let dropped = Arc::new(AtomicBool::new(false));
        let target_drop = dropped.clone();
        let conversions = Arc::new(AtomicUsize::new(0));
        let conversion_count = conversions.clone();
        let worker_source = source.clone();
        let graphics = Thread::new();
        let task = graphics.run_send(target::context()?, move |context| async move {
            let mut target = target::Target::new(context, target_drop).await?;
            let mut cached: Option<Arc<SoftwareFrame>> = None;
            while let Some(size) = incoming.next().await {
                let Some(capture) = worker_source.next_capture(None).await else {
                    continue;
                };
                if cached
                    .as_ref()
                    .is_none_or(|image| image.sequence != capture.sequence)
                {
                    conversion_count.fetch_add(1, Ordering::Release);
                    cached = Some(Arc::new(software::rasterize(&capture)));
                }
                let image = cached.as_ref().unwrap().clone();
                let presented = target.present(&image, size).await?;
                report.unbounded_send((presented, image)).unwrap();
            }
            Ok::<(), String>(())
        });
        let mut display = AppView {
            source,
            requests,
            presentations,
        };
        let busy = display
            .matching(|frame| glyph_top(frame, 0, [255, 255, 85, 255]) == [124, 198, 198, 254, 198])
            .await;
        let busy_pixels = busy.pixels.to_vec();
        leave_busy.send(()).unwrap();
        let waiting = display
            .matching(|frame| glyph_top(frame, 0, [255, 255, 85, 255]) == [252, 102, 102, 124, 96])
            .await;

        // Input is pending and the app stops drawing. VGA still captures while
        // finished output remains unchanged, then resumed demand sees fresh input.
        let converted = conversions.load(Ordering::Acquire);
        let mut sequence = waiting.sequence;
        for _ in 0..4 {
            sequence = display
                .source
                .next_capture(Some(sequence))
                .await
                .unwrap()
                .sequence;
        }
        assert_eq!(conversions.load(Ordering::Acquire), converted);
        assert!(display.presentations.next().now_or_never().is_none());
        let resumed = display.draw().await;
        assert!(resumed.sequence >= sequence);
        key.send(b'X').unwrap();
        assert_eq!(first_exit.await.unwrap(), 7);
        let exited = display
            .matching(|frame| glyph_top(frame, 2, [255, 255, 85, 255]) == [254, 198, 140, 24, 50])
            .await;
        assert_eq!(
            glyph_top(&exited, 0, [255, 255, 85, 255]),
            [252, 102, 102, 124, 96]
        );
        let final_pixels = exited.pixels.to_vec();
        let converted = conversions.load(Ordering::Acquire);
        display
            .source
            .next_capture(Some(exited.sequence))
            .await
            .unwrap();
        assert_eq!(conversions.load(Ordering::Acquire), converted);
        assert!(display.presentations.next().now_or_never().is_none());

        // Reuse this DOS environment, capture source, graphics worker and display.
        launch_next.send(()).unwrap();
        assert_eq!(second_exit.await.unwrap(), 9);
        let second = display
            .matching(|frame| glyph_top(frame, 0, [255, 255, 255, 255]) == [124, 198, 192, 124, 6])
            .await;
        assert!(second.sequence > exited.sequence);
        assert_ne!(&*second.pixels, final_pixels);
        assert_eq!(&*busy.pixels, busy_pixels);
        assert_eq!(&*exited.pixels, final_pixels);

        // DOS shutdown only closes capture production. The fixture owns its
        // surface and can keep displaying the final captured state afterwards.
        assert_eq!(
            execution.stop().await.map_err(|e| e.to_string())?,
            None::<()>
        );
        let last = display.source.current_capture().unwrap();
        assert!(display
            .source
            .next_capture(Some(last.sequence))
            .await
            .is_none());
        assert!(!dropped.load(Ordering::Acquire));
        let after_shutdown = display.draw().await;
        assert_eq!(after_shutdown.sequence, last.sequence);
        let converted = conversions.load(Ordering::Acquire);
        let repeated = display.draw().await;
        assert!(Arc::ptr_eq(&after_shutdown, &repeated));
        assert_eq!(conversions.load(Ordering::Acquire), converted);
        assert!(!dropped.load(Ordering::Acquire));
        drop(display.requests); // Host shutdown is separate from DOS shutdown.
        task.await.map_err(|e| e.to_string())??;
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(&*busy.pixels, busy_pixels);
        assert_eq!(&*exited.pixels, final_pixels);
        Ok(())
    };
    let ((), result) = join(timers.run(), app).await;
    result
}

/// The fixture app issues explicit redraws and waits for GPU presentation reports.
/// Waiting for a matching capture here is test observation, not a rendering clock.
struct AppView {
    source: FrameSource,
    requests: mpsc::Sender<[u32; 2]>,
    presentations: mpsc::UnboundedReceiver<(target::Presented, Arc<SoftwareFrame>)>,
}

impl AppView {
    async fn draw(&mut self) -> Arc<SoftwareFrame> {
        request(&mut self.requests, [640, 480]);
        let (presented, frame) = self.presentations.next().await.expect("worker is live");
        assert_eq!(frame.sequence, presented.sequence);
        for (actual, expected) in presented.pixel.into_iter().zip(&frame.pixels[..4]) {
            assert!(actual.abs_diff(*expected) <= 1);
        }
        frame
    }

    async fn matching(&mut self, matches: impl Fn(&SoftwareFrame) -> bool) -> Arc<SoftwareFrame> {
        loop {
            let frame = self.draw().await;
            if matches(&frame) {
                return frame;
            }
            self.source
                .next_capture(Some(frame.sequence))
                .await
                .unwrap();
        }
    }
}
