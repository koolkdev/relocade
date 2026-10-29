use super::{guest, request, target::read_first_pixel};
use futures_channel::{mpsc, oneshot};
use futures_util::{
    future::{join, select, Either},
    FutureExt, StreamExt,
};
use relocade_machine::{timer_service, worker::Thread};
use relocade_renderer::{fit_image, Image, Rect, Renderer};
use relocade_vga::{CapturedFrame, VgaImage, VgaRenderer};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

enum GraphicsEvent {
    Capture(Option<Arc<CapturedFrame>>),
    Redraw(Option<[u32; 2]>),
}

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
        let (ready, initialized) = oneshot::channel();
        let uploads = Arc::new(AtomicUsize::new(0));
        let upload_count = uploads.clone();
        let renders = Arc::new(AtomicUsize::new(0));
        let render_count = renders.clone();
        let worker_source = source.clone();
        let graphics = Thread::new();
        let task = graphics.run_send((), move |()| async move {
            let instance = wgpu::Instance::default();
            let adapter = instance.request_adapter(&Default::default()).await.unwrap();
            let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
            let mut vga_renderer = VgaRenderer::new(&device).unwrap();
            let mut renderer = Renderer::new(device.clone(), Image::FORMAT).unwrap();
            let mut barrier = Some((selected, allowed));
            let mut ready = Some(ready);
            let mut uploaded_sequence = None;
            let mut source_closed = false;
            let mut cached: Option<VgaImage> = None;
            loop {
                let event = {
                    let next = async {
                        if source_closed {
                            std::future::pending().await
                        } else {
                            worker_source.next_capture(uploaded_sequence).await
                        }
                    };
                    match select(Box::pin(next), incoming.next()).await {
                        Either::Left((capture, _)) => GraphicsEvent::Capture(capture),
                        Either::Right((request, _)) => GraphicsEvent::Redraw(request),
                    }
                };
                let size = match event {
                    GraphicsEvent::Capture(Some(capture)) => {
                        vga_renderer.upload(&queue, &capture);
                        queue.submit([]); // Uploads run independently of redraw requests.
                        uploaded_sequence = Some(capture.sequence);
                        let count = upload_count.fetch_add(1, Ordering::Release) + 1;
                        if count == 2 {
                            ready.take().unwrap().send(()).unwrap();
                        }
                        continue;
                    }
                    GraphicsEvent::Capture(None) => {
                        source_closed = true;
                        continue;
                    }
                    GraphicsEvent::Redraw(Some(size)) => size,
                    GraphicsEvent::Redraw(None) => break,
                };
                if let Some((selected, allowed)) = barrier.take() {
                    selected.send(uploaded_sequence.unwrap()).unwrap();
                    allowed.await.unwrap(); // Hold graphics work, not guest execution.
                }
                let mut encoder = device.create_command_encoder(&Default::default());
                if cached
                    .as_ref()
                    .is_none_or(|image| Some(image.sequence) != uploaded_sequence)
                {
                    cached = vga_renderer.encode(&mut encoder);
                    render_count.fetch_add(1, Ordering::Release);
                }
                let frame = cached.as_ref().unwrap();
                let output = Image::new(&device, size, (1, 1)).unwrap();
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: output.view(),
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    renderer.draw_image(
                        &mut pass,
                        &frame.image,
                        fit_image(&frame.image, Rect::from_size(size)),
                    );
                }
                queue.submit([encoder.finish()]);
                // Only this assertion reads pixels back. Decoding and drawing
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

        // Input uploads continue while decoding and presentation are idle.
        initialized.await.unwrap();
        let mut capture = source.current_capture().unwrap();
        let before = progress.load(Ordering::Acquire);
        for _ in 0..5 {
            capture = source.next_capture(Some(capture.sequence)).await.unwrap();
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(renders.load(Ordering::Acquire), 0);
        assert!(uploads.load(Ordering::Acquire) >= 2);
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
        let mut last_decode_count = None;
        for size in [[640, 480], [320, 240]] {
            request(&mut requests, size);
            let retained = reports.next().await.unwrap();
            assert_eq!(retained.sequence, last.sequence);
            assert_eq!(retained.time, last.time);
            assert_eq!(retained.size, size);
            assert_eq!(retained.pixel, first.pixel);
            let count = renders.load(Ordering::Acquire);
            if let Some(previous) = last_decode_count {
                assert_eq!(count, previous);
            }
            last_decode_count = Some(count);
        }
        drop(requests);
        task.await.map_err(|e| e.to_string())?;
        Ok(())
    };
    let ((), result) = join(timers.run(), app).await;
    result
}
