use super::{guest, request, target::read_first_pixel};
use futures_channel::{mpsc, oneshot};
use futures_util::{future::join, FutureExt, StreamExt};
use relocade_machine::{timer_service, worker::Thread};
use relocade_renderer::{Image, ImageBlitter};
use relocade_vga::VgaRenderer;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

struct Report {
    sequence: u64,
    time: Duration,
    size: [u32; 2],
    pixel: [u8; 4],
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn gpu_workers() -> Result<(), String> {
    let (clock, timers) = timer_service();
    let app = async move {
        let guest::Guest {
            execution,
            source,
            progress,
        } = guest::spawn(clock).await;
        let (mut requests, mut incoming) = mpsc::channel::<[u32; 2]>(0);
        let (selected, selection) = oneshot::channel();
        let (release, allowed) = oneshot::channel();
        let (report, mut reports) = mpsc::unbounded();
        let renders = Arc::new(AtomicUsize::new(0));
        let render_count = renders.clone();
        let worker_source = source.clone();
        let graphics = Thread::new();
        let task = graphics.run_send((), move |()| async move {
            let instance = wgpu::Instance::default();
            let adapter = instance.request_adapter(&Default::default()).await.unwrap();
            let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
            let mut renderer = VgaRenderer::new(device.clone(), queue.clone()).unwrap();
            let mut blitter =
                ImageBlitter::new(device.clone(), queue.clone(), Image::FORMAT).unwrap();
            let mut barrier = Some((selected, allowed));
            while let Some(size) = incoming.next().await {
                let capture = worker_source.current_capture().unwrap();
                if let Some((selected, allowed)) = barrier.take() {
                    selected.send(capture.sequence).unwrap();
                    allowed.await.unwrap(); // Hold graphics work, not guest execution.
                }
                let frame = renderer.render(&capture);
                render_count.fetch_add(1, Ordering::Release);
                let output = Image::new(&device, size, (1, 1)).unwrap();
                blitter.blit(&frame.image, output.view(), size).unwrap();
                // Only this assertion reads pixels back. The renderer and blitter
                // normally keep the entire image on the GPU.
                let pixel = read_first_pixel(&device, &queue, output.texture()).await;
                report
                    .unbounded_send(Report {
                        sequence: frame.sequence,
                        time: frame.time,
                        size,
                        pixel,
                    })
                    .unwrap();
            }
        });

        // Merely producing captures does not ask the graphics owner to render.
        let mut capture = source.current_capture().unwrap();
        let before = progress.load(Ordering::Acquire);
        for _ in 0..5 {
            capture = source.next_capture(Some(capture.sequence)).await.unwrap();
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(renders.load(Ordering::Acquire), 0);
        request(&mut requests, [64, 48]);
        let selected = selection.await.unwrap();

        // The fixture coalesces requests in one slot. While decoding is held,
        // the guest keeps writing text and reading the VGA status port.
        let before = progress.load(Ordering::Acquire);
        for _ in 0..10 {
            for _ in 0..100 {
                request(&mut requests, [96, 72]);
            }
            capture = source.next_capture(Some(capture.sequence)).await.unwrap();
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(renders.load(Ordering::Acquire), 0);
        release.send(()).unwrap();
        let first = reports.next().await.unwrap();
        assert_eq!(first.sequence, selected);
        assert_eq!(first.size, [64, 48]);
        assert_eq!(first.pixel, [255, 255, 85, 255]);
        let second = reports.next().await.unwrap();
        assert!(second.sequence >= capture.sequence);
        assert!(second.time > first.time);
        assert_eq!(second.size, [96, 72]);
        assert_eq!(second.pixel, first.pixel);

        capture = source.current_capture().unwrap();
        let before = progress.load(Ordering::Acquire);
        for _ in 0..5 {
            capture = source.next_capture(Some(capture.sequence)).await.unwrap();
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(renders.load(Ordering::Acquire), 2);
        assert!(reports.next().now_or_never().is_none());

        // Capture closure does not terminate the separately owned GPU resources.
        assert_eq!(execution.stop().await.map_err(|e| e.to_string())?, None);
        let last = source.current_capture().unwrap();
        assert!(source.next_capture(Some(last.sequence)).await.is_none());
        for size in [[640, 480], [320, 240]] {
            request(&mut requests, size);
            let retained = reports.next().await.unwrap();
            assert_eq!(retained.sequence, last.sequence);
            assert_eq!(retained.time, last.time);
            assert_eq!(retained.size, size);
            assert_eq!(retained.pixel, first.pixel);
        }
        drop(requests);
        task.await.map_err(|e| e.to_string())?;
        Ok(())
    };
    let ((), result) = join(timers.run(), app).await;
    result
}
