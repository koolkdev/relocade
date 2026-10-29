#![cfg(any(target_os = "linux", target_arch = "wasm32"))]

use futures_channel::oneshot;
use relocade_app::{
    graphics::{Surface, SurfaceSetup},
    winit::{
        application::ApplicationHandler,
        dpi::PhysicalSize,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoopProxy},
        window::{WindowAttributes, WindowId},
    },
    worker::Thread,
    Window,
};
use relocade_renderer::{fit_image, Image, Rect, Renderer};

struct Ready {
    setup: SurfaceSetup,
    size: PhysicalSize<u32>,
    events: EventLoopProxy<()>,
    #[cfg(target_os = "linux")]
    window_id: u32,
}

struct App {
    window: Option<Window>,
    events: EventLoopProxy<()>,
    ready: Option<oneshot::Sender<Ready>>,
    closed: Option<oneshot::Sender<()>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Window::new(event_loop, WindowAttributes::default()).unwrap();
        window.winit().request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let window = self.window.as_mut().unwrap();
        if !window.handle_event(id, &event) {
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) && window.state().drawable() {
            if let Some(ready) = self.ready.take() {
                let setup = window.surface_setup(event_loop).unwrap();
                ready
                    .send(Ready {
                        setup,
                        size: window.state().size,
                        events: self.events.clone(),
                        #[cfg(target_os = "linux")]
                        window_id: native::window_id(window),
                    })
                    .ok()
                    .unwrap();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        event_loop.exit();
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.window = None;
        self.closed.take().unwrap().send(()).unwrap();
    }
}

async fn draw(
    setup: SurfaceSetup,
    size: PhysicalSize<u32>,
    presented: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut surface = Surface::new(setup).await?;
    assert!(surface.acquire(PhysicalSize::new(0, size.height)).is_err());
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

async fn render_and_close(ready: oneshot::Receiver<Ready>, closed: oneshot::Receiver<()>) {
    let Ready {
        setup,
        size,
        events,
        #[cfg(target_os = "linux")]
        window_id,
    } = ready.await.unwrap();
    let thread = Thread::new();
    let (presented, visible) = oneshot::channel();
    let (release, observed) = oneshot::channel();
    let task = thread.run_send(setup, move |setup| draw(setup, size, presented, observed));
    let presented = visible.await.is_ok();
    #[cfg(target_os = "linux")]
    let pixels_match = presented && native::presented_pixels_match(window_id, size);
    let _ = release.send(());
    let result = task.await;
    drop(thread); // The GPU owner has finished before the main owner releases its window.
    events.send_event(()).unwrap();
    closed.await.unwrap();
    result.unwrap().unwrap();
    assert!(presented);
    #[cfg(target_os = "linux")]
    assert!(
        pixels_match,
        "presented window pixels did not match the image"
    );
}

#[cfg(target_os = "linux")]
#[path = "surface/native.rs"]
mod native;

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires X11 and a graphics adapter; run under Xvfb"]
fn native_surface_runs_on_a_separate_graphics_worker() {
    use relocade_app::winit::{event_loop::EventLoop, platform::x11::EventLoopBuilderExtX11};
    let (ready, started) = oneshot::channel();
    let (closed, finished) = oneshot::channel();
    let host = std::thread::spawn(move || {
        let event_loop = EventLoop::builder()
            .with_x11()
            .with_any_thread(true)
            .build()
            .unwrap();
        let mut app = App {
            window: None,
            events: event_loop.create_proxy(),
            ready: Some(ready),
            closed: Some(closed),
        };
        event_loop.run_app(&mut app).unwrap();
    });
    pollster::block_on(render_and_close(started, finished));
    host.join().unwrap();
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn browser_surface_runs_on_a_separate_graphics_worker() {
    let (ready, started) = oneshot::channel();
    let (closed, finished) = oneshot::channel();
    relocade_app::run(|events| App {
        window: None,
        events,
        ready: Some(ready),
        closed: Some(closed),
    })
    .unwrap();
    render_and_close(started, finished).await;
}
