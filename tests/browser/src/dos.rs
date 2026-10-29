use crate::vga::glyph_top;
use futures_channel::oneshot;
use futures_util::future::join;
use relocade_dos::Dos;
use relocade_machine::{timer_service, worker::Thread, ExecutionWorker};
use relocade_vga::{Display, VideoFrame};
use std::{future::pending, io::Write, sync::Arc};

// The first program writes VGA while spinning on a fixture-controlled RAM flag,
// prints P, waits for a DOS echoed key, writes Z, then exits with code 7.
const FIRST_PROGRAM: &[u8] = &[
    0xb8, 0x00, 0xb8, 0x8e, 0xc0, // ES = B800.
    0x26, 0xc7, 0x06, 0, 0, b'A', 0x1e, // Cell 0 = A.
    0x26, 0xff, 0x06, 2, 0, // Loop: INC word ES:[2].
    0x66, 0xff, 0x06, 4, 4, // INC dword DS:[0404], CPU progress counter.
    0x80, 0x3e, 0, 4, 0, // CMP byte DS:[0400],0.
    0x74, 0xef, // JE loop.
    0xb4, 2, 0xb2, b'P', 0xcd, 0x21, // DOS print P.
    0xb4, 1, 0xcd, 0x21, // DOS read and echo.
    0x26, 0xc7, 0x06, 4, 0, b'Z', 0x1e, // Last VGA write before exit.
    0xb8, 7, 0x4c, 0xcd, 0x21,
];
const SECOND_PROGRAM: &[u8] = &[
    0xb8, 0x00, 0xb8, 0x8e, 0xc0, 0x26, 0xc7, 0x06, 0, 0, b'S', 0x2f, 0xb8, 9, 0x4c, 0xcd, 0x21,
];

async fn next_matching(
    display: &Display,
    after: &mut Option<u64>,
    matches: impl Fn(&VideoFrame) -> bool,
) -> Arc<VideoFrame> {
    loop {
        let frame = display
            .next_frame(*after)
            .await
            .expect("session video is live");
        *after = Some(frame.sequence);
        if matches(&frame) {
            return frame;
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn dos_workers() -> Result<(), String> {
    let (video, video_ready) = oneshot::channel();
    let (slow, slow_started) = oneshot::channel();
    let (resume, render_resume) = oneshot::channel();
    let (leave_busy, busy_observed) = oneshot::channel();
    let (key, key_input) = oneshot::channel();
    let (first_done, first_exit) = oneshot::channel();
    let (launch_next, next_launch) = oneshot::channel();
    let (second_done, second_exit) = oneshot::channel();
    let (rendered, rendering_done) = oneshot::channel();

    let (clock, timers) = timer_service();
    let execution = ExecutionWorker::spawn(clock, async move |runner| {
        let mut dos = Dos::new(runner).await.unwrap();
        let display = dos.display();
        assert!(video.send(display.clone()).is_ok());
        dos.launch_com(FIRST_PROGRAM, Default::default()).unwrap();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        let memory = dos.cpu().memory();
        let mut input = dos.stdin();
        let fixture_input = async {
            // The renderer stops consuming; execution and frame production continue.
            let mut sequence = slow_started.await.unwrap();
            let before = memory.view().read_u32(base + 0x404).unwrap();
            for _ in 0..4 {
                sequence = display.next_frame(Some(sequence)).await.unwrap().sequence;
            }
            let after = memory.view().read_u32(base + 0x404).unwrap();
            assert!(after.wrapping_sub(before) > 0);
            resume.send(sequence).unwrap();
            busy_observed.await.unwrap();
            memory.view().write_u8(base + 0x400, 1).unwrap();
            // Only the fixture bridges a Rust message to the existing local stdin.
            input.write_all(&[key_input.await.unwrap()]).unwrap();
        };
        let (result, ()) = join(dos.run(runner), fixture_input).await;
        assert_eq!(dos.screen().cells[1].character, b'X');
        first_done.send(result.unwrap()).unwrap();
        // Process completion leaves this environment and its video timing alive.
        next_launch.await.unwrap();
        dos.launch_com(SECOND_PROGRAM, Default::default()).unwrap();
        second_done.send(dos.run(runner).await.unwrap()).unwrap();
        pending::<()>().await;
    });

    let graphics = Thread::new();
    let rendering = graphics.run_send((), move |()| async move {
        let display = video_ready.await.unwrap();
        let mut after = None;
        let busy = next_matching(&display, &mut after, |frame| {
            glyph_top(frame, 0, [255, 255, 85, 255]) == [124, 198, 198, 254, 198]
        })
        .await;
        let busy_pixels = busy.pixels.to_vec();
        assert_eq!(busy_pixels.len(), 640 * 200 * 4);
        slow.send(busy.sequence).unwrap();
        let completed_sequence = render_resume.await.unwrap();
        // Resume on the consumer's schedule and sample the newest completed output.
        let newest = display.current_frame().unwrap();
        assert!(newest.sequence >= completed_sequence);
        after = Some(newest.sequence);
        leave_busy.send(()).unwrap();

        let waiting = next_matching(&display, &mut after, |frame| {
            glyph_top(frame, 0, [255, 255, 85, 255]) == [252, 102, 102, 124, 96]
        })
        .await;
        assert_eq!(waiting.pixels.len(), busy_pixels.len());
        for _ in 0..2 {
            let next = display.next_frame(after).await.unwrap();
            assert!(next.sequence > after.unwrap());
            assert_eq!(
                glyph_top(&next, 0, [255, 255, 85, 255]),
                [252, 102, 102, 124, 96]
            );
            after = Some(next.sequence);
        }
        key.send(b'X').unwrap();
        assert_eq!(first_exit.await.unwrap(), 7);
        let exited = next_matching(&display, &mut after, |frame| {
            glyph_top(frame, 2, [255, 255, 85, 255]) == [254, 198, 140, 24, 50]
        })
        .await;
        assert_eq!(
            glyph_top(&exited, 0, [255, 255, 85, 255]),
            [252, 102, 102, 124, 96]
        );
        // DOS echoes X with the attribute present at cell 1; its changing value
        // belongs to the busy-loop fixture, so verify the echoed byte on the CPU.
        let final_pixels = exited.pixels.to_vec();
        assert_ne!(final_pixels, busy_pixels);
        let idle = display.next_frame(after).await.unwrap();
        assert!(idle.sequence > exited.sequence);
        assert_eq!(
            glyph_top(&idle, 2, [255, 255, 85, 255]),
            [254, 198, 140, 24, 50]
        );
        after = Some(idle.sequence);

        launch_next.send(()).unwrap();
        assert_eq!(second_exit.await.unwrap(), 9);
        let second = next_matching(&display, &mut after, |frame| {
            glyph_top(frame, 0, [255, 255, 255, 255]) == [124, 198, 192, 124, 6]
        })
        .await;
        assert_ne!(&*second.pixels, final_pixels);
        assert_eq!(&*busy.pixels, busy_pixels);
        assert_eq!(&*exited.pixels, final_pixels);
        rendered.send(()).unwrap();

        while let Some(frame) = display.next_frame(after).await {
            after = Some(frame.sequence);
        }
        let retained = display.current_frame().unwrap();
        assert_eq!(retained.sequence, after.unwrap());
        assert_eq!(
            glyph_top(&retained, 0, [255, 255, 255, 255]),
            [124, 198, 192, 124, 6]
        );
        assert_eq!(retained.pixels.len(), 640 * 200 * 4);
    });
    let control = async {
        rendering_done.await.unwrap();
        assert_eq!(
            execution.stop().await.map_err(|error| error.to_string())?,
            None::<()>
        );
        rendering.await.map_err(|error| error.to_string())?;
        Ok(())
    };
    let ((), result) = join(timers.run(), control).await;
    result
}
