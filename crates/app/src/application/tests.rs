use super::*;
use futures_channel::mpsc;
use relocade_frontend::{
    display::DisplayRenderer,
    keyboard::{self, KeyboardEvent, ModifiersState},
};
use relocade_renderer::Image;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    time::Duration,
};
#[cfg(target_os = "linux")]
use winit::dpi::PhysicalSize;
use winit::{application::ApplicationHandler, event::WindowEvent, window::WindowId};

#[derive(Default)]
struct Counts {
    draws: AtomicUsize,
    updates: AtomicUsize,
    release: AtomicBool,
    dropped: AtomicBool,
}

struct Display {
    image: Image,
    updates: mpsc::UnboundedReceiver<()>,
    counts: Arc<Counts>,
}

impl DisplayRenderer for Display {
    async fn update(&mut self, _: &wgpu::Queue) -> Option<bool> {
        self.updates.recv().await.ok()?;
        self.counts.updates.fetch_add(1, SeqCst);
        Some(true)
    }
    fn encode_image(&mut self, _: &mut wgpu::CommandEncoder) -> Option<&Image> {
        self.counts.draws.fetch_add(1, SeqCst);
        while !self.counts.release.load(SeqCst) {
            std::hint::spin_loop();
        }
        Some(&self.image)
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        self.counts.dropped.store(true, SeqCst);
    }
}

async fn wait_for(condition: impl Fn() -> bool) {
    for _ in 0..2000 {
        if condition() {
            return;
        }
        futures_timer::Delay::new(Duration::from_millis(2)).await;
    }
    panic!("application fixture timed out");
}

#[derive(Clone, Copy)]
enum Startup {
    Ready,
    DisplayError,
    CloseBeforeAttach,
}

enum Action {
    Hide(bool),
    Resize,
    Close,
    Modifiers,
    FocusLost,
}
type Actions = Rc<RefCell<VecDeque<(Action, oneshot::Sender<()>)>>>;

struct Fixture {
    application: Application,
    actions: Actions,
}

impl ApplicationHandler<Event> for Fixture {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        self.application.resumed(events);
    }
    fn window_event(&mut self, events: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.application.window_event(events, id, event);
    }
    fn user_event(&mut self, events: &ActiveEventLoop, event: Event) {
        self.application.user_event(events, event);
        while let Some((action, done)) = self.actions.borrow_mut().pop_front() {
            let window = self
                .application
                .window
                .as_ref()
                .expect("cleanup retains the window");
            let id = window.winit().id();
            match action {
                Action::Hide(hidden) => {
                    self.application
                        .window_event(events, id, WindowEvent::Occluded(hidden))
                }
                Action::Close => {
                    self.application
                        .window_event(events, id, WindowEvent::CloseRequested)
                }
                Action::Modifiers => self.application.window_event(
                    events,
                    id,
                    WindowEvent::ModifiersChanged(winit::keyboard::ModifiersState::CONTROL.into()),
                ),
                Action::FocusLost => {
                    self.application
                        .window_event(events, id, WindowEvent::Focused(false))
                }
                Action::Resize => {
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = window
                        .winit()
                        .request_inner_size(PhysicalSize::new(320, 240));
                    #[cfg(target_arch = "wasm32")]
                    {
                        use winit::platform::web::WindowExtWebSys;
                        let canvas = window.winit().canvas().unwrap();
                        canvas.style().set_property("width", "320px").unwrap();
                        canvas.style().set_property("height", "240px").unwrap();
                    }
                }
            }
            done.send(()).unwrap();
        }
    }
}

fn fixture(
    events: EventLoopProxy<Event>,
    attributes: WindowAttributes,
    completed: oneshot::Sender<Result<(), String>>,
    startup: Startup,
) -> Fixture {
    let actions = Actions::default();
    let requests = actions.clone();
    let counts = Arc::new(Counts::default());
    let (updates, incoming) = mpsc::unbounded();
    let cleaned = Rc::new(Cell::new(false));
    let cleanup_observed = cleaned.clone();
    let application = Application::new(
        events.clone(),
        attributes,
        move |mut host| async move {
            // This Rc and the thread ID must remain on the winit owner across awaits.
            let main = Rc::new(std::thread::current().id());
            let main_id = *main;
            let display_counts = counts.clone();
            let action = |action| {
                let (done, completed) = oneshot::channel();
                requests.borrow_mut().push_back((action, done));
                events.send_event(Event::Wake).unwrap();
                async { completed.await.unwrap() }
            };
            // A held modifier must survive keyboard attachment after startup.
            action(Action::Modifiers).await;
            let connector = crate::worker::Thread::new();
            let (input, mut keys) = connector
                .run_send((), move |()| async move {
                    assert_ne!(main_id, std::thread::current().id());
                    keyboard::channel()
                })
                .await
                .map_err(|error| error.to_string())?;
            drop(connector);
            if matches!(startup, Startup::CloseBeforeAttach) {
                action(Action::Close).await;
                host.closing().await?;
                assert!(host.attach_keyboard(input).is_err());
                assert!(host
                    .attach_display::<Display>(|_, _| panic!("closed window cannot start graphics"))
                    .is_err());
                cleaned.set(true);
                return Ok(());
            }
            // Execution returns ordinary endpoints; the main-thread host attaches them.
            assert_eq!(main_id, std::thread::current().id());
            host.attach_keyboard(input)?;
            host.attach_display(move |device, queue| {
                assert_ne!(main_id, std::thread::current().id());
                if matches!(startup, Startup::DisplayError) {
                    return Err("fixture display failed".into());
                }
                let image = Image::new(device, [1, 1], (1, 1)).map_err(|e| e.to_string())?;
                image
                    .upload(queue, &[255, 0, 0, 255])
                    .map_err(|e| e.to_string())?;
                Ok(Display {
                    image,
                    updates: incoming,
                    counts: display_counts,
                })
            })?;
            assert_eq!(
                keys.recv().await.unwrap(),
                KeyboardEvent::Modifiers(ModifiersState::CONTROL)
            );
            if matches!(startup, Startup::DisplayError) {
                assert_eq!(host.closing().await.unwrap_err(), "fixture display failed");
                // Closing again during delayed cleanup must retain the window
                // and the original graphics error.
                action(Action::Close).await;
                futures_timer::Delay::new(Duration::from_millis(20)).await;
                cleaned.set(true);
                return Err("fixture cleanup also failed".into());
            }
            action(Action::Modifiers).await;
            assert_eq!(
                keys.recv().await.unwrap(),
                KeyboardEvent::Modifiers(ModifiersState::CONTROL)
            );
            action(Action::FocusLost).await;
            assert_eq!(keys.recv().await.unwrap(), KeyboardEvent::FocusLost);
            wait_for(|| counts.draws.load(SeqCst) > 0).await;
            // Main handles events and timers while the graphics worker is held.
            action(Action::Hide(true)).await;
            futures_timer::Delay::new(Duration::from_millis(20)).await;
            counts.release.store(true, SeqCst);
            futures_timer::Delay::new(Duration::from_millis(40)).await;
            let draws = counts.draws.load(SeqCst);
            for _ in 0..20 {
                updates.unbounded_send(()).unwrap();
            }
            wait_for(|| counts.updates.load(SeqCst) == 20).await;
            futures_timer::Delay::new(Duration::from_millis(40)).await;
            assert_eq!(
                counts.draws.load(SeqCst),
                draws,
                "hidden output must not draw"
            );
            action(Action::Hide(false)).await;
            wait_for(|| counts.draws.load(SeqCst) > draws).await;
            futures_timer::Delay::new(Duration::from_millis(20)).await;
            let draws = counts.draws.load(SeqCst);
            action(Action::Resize).await;
            wait_for(|| counts.draws.load(SeqCst) > draws).await;
            assert_eq!(
                counts.updates.load(SeqCst),
                20,
                "resize needs no source update"
            );
            drop(updates);
            action(Action::Close).await;
            host.closing().await?;
            // The callback has not released the window while lifecycle cleanup awaits.
            wait_for(|| counts.dropped.load(SeqCst)).await;
            futures_timer::Delay::new(Duration::from_millis(20)).await;
            assert_eq!(*main, std::thread::current().id());
            cleaned.set(true);
            Ok(())
        },
        move |result| {
            assert!(
                cleanup_observed.get(),
                "completion must follow lifecycle cleanup"
            );
            completed.send(result).unwrap();
        },
    );
    Fixture {
        application,
        actions,
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires X11 and a graphics adapter; run under Xvfb"]
fn main_hosts_display_and_awaits_cleanup() {
    use winit::{
        event_loop::EventLoop,
        platform::{run_on_demand::EventLoopExtRunOnDemand, x11::EventLoopBuilderExtX11},
    };
    let mut event_loop = EventLoop::<Event>::with_user_event()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    for startup in [
        Startup::Ready,
        Startup::DisplayError,
        Startup::CloseBeforeAttach,
    ] {
        let (completed, finished) = oneshot::channel();
        let mut app = fixture(
            event_loop.create_proxy(),
            WindowAttributes::default().with_inner_size(PhysicalSize::new(640, 480)),
            completed,
            startup,
        );
        event_loop.run_app_on_demand(&mut app).unwrap();
        let result = pollster::block_on(finished).unwrap();
        assert_eq!(
            result,
            if matches!(startup, Startup::DisplayError) {
                Err("fixture display failed".into())
            } else {
                Ok(())
            }
        );
        assert!(app.application.window.is_none());
    }
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn main_hosts_display_and_awaits_cleanup() {
    use wasm_bindgen::JsCast;
    use winit::platform::web::WindowAttributesExtWebSys;
    for startup in [
        Startup::Ready,
        Startup::DisplayError,
        Startup::CloseBeforeAttach,
    ] {
        let canvas: web_sys::HtmlCanvasElement = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .create_element("canvas")
            .unwrap()
            .dyn_into()
            .unwrap();
        canvas.style().set_property("width", "640px").unwrap();
        canvas.style().set_property("height", "480px").unwrap();
        let (completed, finished) = oneshot::channel();
        crate::run(|events| {
            fixture(
                events,
                WindowAttributes::default().with_canvas(Some(canvas.clone())),
                completed,
                startup,
            )
        })
        .unwrap();
        let result = finished.await.unwrap();
        assert_eq!(
            result,
            if matches!(startup, Startup::DisplayError) {
                Err("fixture display failed".into())
            } else {
                Ok(())
            }
        );
        assert!(!canvas.is_connected());
        // Let winit finish exiting before starting the next independent application.
        futures_timer::Delay::new(Duration::ZERO).await;
    }
}
