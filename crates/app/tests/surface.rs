#![cfg(any(target_os = "linux", target_arch = "wasm32"))]

use futures_channel::oneshot;
use relocade_app::{
    graphics::{Surface, SurfaceSetup},
    winit::dpi::PhysicalSize,
    worker::Thread,
};
use relocade_renderer::{fit_image, Image, Rect, Renderer};

async fn draw(
    setup: SurfaceSetup,
    size: PhysicalSize<u32>,
    presented: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut surface = Surface::new(setup).await?;
    assert!(surface.acquire(PhysicalSize::new(0, size.height)).is_err());
    // Retain the same platform surface across two GPU devices, including a
    // configured swapchain. The browser must not transfer its canvas twice.
    drop(surface.acquire(size)?);
    surface.device.destroy();
    let _ = surface.device.poll(wgpu::PollType::wait_indefinitely());
    surface.recreate_device().await?;
    let frame = match surface.acquire(size)? {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        other => return Err(format!("Initial frame unavailable: {other:?}")),
    };
    assert_eq!(
        [frame.texture.width(), frame.texture.height()],
        [size.width, size.height]
    );
    let image = Image::new(&surface.device, [2, 2], (1, 1)).map_err(|e| e.to_string())?;
    image
        .upload(
            &surface.queue,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        )
        .map_err(|e| e.to_string())?;
    let mut renderer =
        Renderer::new(surface.device.clone(), surface.format()).map_err(|e| e.to_string())?;
    let mut encoder = surface.device.create_command_encoder(&Default::default());
    let view = frame.texture.create_view(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
            &image,
            fit_image(&image, Rect::from_size(size.into())),
        );
    }
    surface.queue.submit([encoder.finish()]);
    surface.present(frame);
    // Only the fixture waits for GPU completion; normal presentation does not.
    let (done, completed) = oneshot::channel();
    surface.queue.on_submitted_work_done(move || {
        let _ = done.send(());
    });
    surface
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    completed.await.map_err(|e| e.to_string())?;
    presented
        .send(())
        .map_err(|_| "Presentation observer dropped")?;
    release.await.map_err(|e| e.to_string())?;
    Ok(())
}

#[path = "support/window.rs"]
mod window;

async fn render_and_close() {
    let (ready, close) = window::TestWindow::open().await.split();
    let window::Ready {
        setup,
        size,
        #[cfg(target_os = "linux")]
        window_id,
        ..
    } = ready;
    let thread = Thread::new();
    let (presented, visible) = oneshot::channel();
    let (release, observed) = oneshot::channel();
    let task = thread.run_send(setup, move |setup| draw(setup, size, presented, observed));
    let presented = visible.await.is_ok();
    #[cfg(target_os = "linux")]
    let pixels_match = presented && window::native::presented_pixels_match(window_id, size);
    let _ = release.send(());
    let result = task.await;
    drop(thread);
    close.await;
    result.unwrap().unwrap();
    assert!(presented);
    #[cfg(target_os = "linux")]
    assert!(
        pixels_match,
        "presented window pixels did not match the image"
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires X11 and a graphics adapter; run under Xvfb"]
fn native_surface_runs_on_a_separate_graphics_worker() {
    pollster::block_on(render_and_close());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn browser_surface_runs_on_a_separate_graphics_worker() {
    render_and_close().await;
}
