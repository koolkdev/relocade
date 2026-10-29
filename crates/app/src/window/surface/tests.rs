use super::super::platform;
use super::*;
use crate::worker::Worker;
use std::task::{Context, Poll, Wake};
use winit::{
    application::ApplicationHandler,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::x11::EventLoopBuilderExtX11,
    window::Window,
};

struct Notify(EventLoopProxy<Event>);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        let _ = self.0.send_event(Event::Wake);
    }
}
struct Recovery {
    events: EventLoopProxy<Event>,
    window: Option<Arc<Window>>,
    probe: Option<Worker<Result<(), String>>>,
    outcome: Option<Result<(), String>>,
    deadline: std::time::Instant,
}
impl Recovery {
    fn poll(&mut self, event_loop: &ActiveEventLoop) {
        assert!(
            std::time::Instant::now() < self.deadline,
            "surface recreation timed out"
        );
        let waker = Arc::new(Notify(self.events.clone())).into();
        if let Some(probe) = &mut self.probe {
            if let Poll::Ready(result) = probe.poll(&mut Context::from_waker(&waker)) {
                self.outcome = Some(
                    result
                        .and_then(|result| result.ok_or("probe cancelled".into()))
                        .and_then(|result| result),
                );
                event_loop.exit();
            }
        }
    }
}
impl ApplicationHandler<Event> for Recovery {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_inner_size(winit::dpi::PhysicalSize::new(96, 64)),
                )
                .unwrap(),
        );
        let setup = SurfaceSetup::new(window.clone(), event_loop).unwrap();
        self.window = Some(window);
        let events = self.events.clone();
        self.probe = Some(Worker::spawn(setup, move |setup| async move {
            let mut surface = WindowSurface::new(setup, &events).await?;
            draw(&mut surface, &events, wgpu::Color::RED).await?;
            // Invoke the private operation used by Lost. This checks recreation
            // and its real main/worker transfer, not driver loss detection.
            surface.recreate(&events).await?;
            draw(&mut surface, &events, wgpu::Color::GREEN).await
        }));
        self.poll(event_loop);
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::RecreateSurface {
                instance,
                window,
                reply,
            } => {
                assert!(self
                    .window
                    .as_ref()
                    .is_some_and(|owned| Arc::ptr_eq(owned, &window)));
                let _ = reply.send(platform::create_surface(&instance, window));
            }
            Event::Failed(error) => panic!("{error}"),
            _ => {}
        }
        self.poll(event_loop);
    }
    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        _event: winit::event::WindowEvent,
    ) {
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
        self.poll(event_loop);
    }
}

async fn draw(
    surface: &mut WindowSurface,
    events: &EventLoopProxy<Event>,
    color: wgpu::Color,
) -> Result<(), String> {
    let frame = match surface.acquire([96, 64], events).await? {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        other => return Err(format!("Unexpected acquisition: {other:?}")),
    };
    let view = frame.texture.create_view(&Default::default());
    let mut encoder = surface.device.create_command_encoder(&Default::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    }));
    surface.queue.submit([encoder.finish()]);
    surface.present(frame);
    Ok(())
}

#[test]
#[ignore = "requires an X11 display, run under Xvfb"]
fn recreates_surface_through_main_and_draws_on_the_same_graphics_owner() {
    let events = EventLoop::<Event>::with_user_event()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    let mut recovery = Recovery {
        events: events.create_proxy(),
        window: None,
        probe: None,
        outcome: None,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
    };
    events.run_app(&mut recovery).unwrap();
    recovery.outcome.expect("graphics probe completed").unwrap();
}
