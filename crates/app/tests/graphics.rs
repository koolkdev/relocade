#![cfg(any(target_os = "linux", target_arch = "wasm32"))]

use futures_channel::{mpsc, oneshot};
use relocade_app::{graphics::Graphics, winit::dpi::PhysicalSize};
use relocade_frontend::display::DisplayRenderer;
use relocade_renderer::Image;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        Arc,
    },
    time::Duration,
};

#[path = "support/window.rs"]
mod window;

#[derive(Default)]
struct Counts {
    updates: AtomicUsize,
    draws: AtomicUsize,
    decodes: AtomicUsize,
    notifications: AtomicUsize,
    source_closed: AtomicBool,
    flooding: AtomicBool,
    callback_completed: AtomicBool,
    released: AtomicBool,
    dropped: AtomicBool,
}

struct TestDisplay {
    device: wgpu::Device,
    input: Image,
    image: Image,
    incoming: mpsc::UnboundedReceiver<bool>,
    counts: Arc<Counts>,
    blocked: Option<oneshot::Sender<()>>,
    dirty: bool,
    callback_requested: bool,
    owner: std::thread::ThreadId,
}

impl DisplayRenderer for TestDisplay {
    async fn update(&mut self, queue: &wgpu::Queue) -> Option<bool> {
        if self.counts.flooding.load(SeqCst) {
            if !self.callback_requested {
                let counts = self.counts.clone();
                queue.on_submitted_work_done(move || {
                    counts.callback_completed.store(true, SeqCst);
                });
                self.callback_requested = true;
            }
            // Native dispatches callbacks through poll; WebGPU delivers them as
            // browser tasks, which must run while this source remains ready.
            self.device.poll(wgpu::PollType::Poll).unwrap();
            self.counts.updates.fetch_add(1, SeqCst);
            return Some(false);
        }
        let Ok(visible_change) = self.incoming.recv().await else {
            self.counts.source_closed.store(true, SeqCst);
            return None;
        };
        self.input
            .upload(
                queue,
                &[
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                ],
            )
            .unwrap();
        self.dirty |= visible_change;
        self.counts.updates.fetch_add(1, SeqCst);
        Some(visible_change)
    }

    fn encode_image(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<&Image> {
        self.counts.draws.fetch_add(1, SeqCst);
        if let Some(blocked) = self.blocked.take() {
            blocked.send(()).unwrap();
            // Deliberately stall only graphics. Main must keep processing demand.
            while !self.counts.released.load(SeqCst) {
                std::hint::spin_loop();
            }
        }
        if self.dirty {
            encoder.copy_texture_to_texture(
                self.input.texture().as_image_copy(),
                self.image.texture().as_image_copy(),
                self.input.texture().size(),
            );
            self.dirty = false;
            self.counts.decodes.fetch_add(1, SeqCst);
        }
        Some(&self.image)
    }
}

impl Drop for TestDisplay {
    fn drop(&mut self) {
        assert_eq!(self.owner, std::thread::current().id());
        self.counts.dropped.store(true, SeqCst);
    }
}

async fn wait_until(ready: impl Fn() -> bool) {
    for _ in 0..2000 {
        if ready() {
            return;
        }
        futures_timer::Delay::new(Duration::from_millis(2)).await;
    }
    panic!("graphics fixture timed out");
}

async fn exercise_graphics() {
    let (ready, close_window) = window::TestWindow::open().await.split();
    let (input, incoming) = mpsc::unbounded();
    let (blocked, stalled) = oneshot::channel();
    let counts = Arc::new(Counts::default());
    let display_counts = counts.clone();
    let notifications = counts.clone();
    let mut graphics = Graphics::spawn(
        ready.setup,
        move |device, _queue| {
            Ok(TestDisplay {
                device: device.clone(),
                input: Image::new(device, [2, 2], (1, 1)).map_err(|e| e.to_string())?,
                image: Image::new(device, [2, 2], (1, 1)).map_err(|e| e.to_string())?,
                incoming,
                counts: display_counts,
                blocked: Some(blocked),
                dirty: false,
                callback_requested: false,
                owner: std::thread::current().id(),
            })
        },
        move || {
            notifications.notifications.fetch_add(1, SeqCst);
        },
    );
    for _ in 0..20 {
        input.unbounded_send(true).unwrap();
    }
    wait_until(|| counts.updates.load(SeqCst) == 20).await;
    assert_eq!(counts.draws.load(SeqCst), 0, "updates alone must not draw");
    assert_eq!(
        counts.notifications.load(SeqCst),
        1,
        "notifications must coalesce too"
    );

    graphics.request_redraw(ready.size);
    stalled.await.unwrap();
    // Cancelling a completion wait keeps the worker and its controls usable.
    assert!(futures_lite::future::poll_once(graphics.finished())
        .await
        .is_none());
    for width in 1..=1000 {
        graphics.request_redraw(PhysicalSize::new(width, 200));
    }
    graphics.request_redraw(PhysicalSize::new(0, 0));
    assert_eq!(
        counts.draws.load(SeqCst),
        1,
        "main progresses while graphics is stalled"
    );
    counts.released.store(true, SeqCst);
    futures_timer::Delay::new(Duration::from_millis(40)).await;
    assert_eq!(
        counts.draws.load(SeqCst),
        1,
        "the latest pending size suppresses drawing"
    );
    graphics.request_redraw(ready.size);
    wait_until(|| counts.draws.load(SeqCst) == 2).await;
    futures_timer::Delay::new(Duration::from_millis(40)).await;
    assert_eq!(counts.draws.load(SeqCst), 2, "no queued resize backlog");
    assert_eq!(counts.decodes.load(SeqCst), 1, "same input is encoded once");

    // Synchronizing non-visible data must not schedule presentation.
    let notifications = counts.notifications.load(SeqCst);
    input.unbounded_send(false).unwrap();
    wait_until(|| counts.updates.load(SeqCst) == 21).await;
    futures_timer::Delay::new(Duration::from_millis(40)).await;
    assert_eq!(counts.notifications.load(SeqCst), notifications);
    assert_eq!(counts.draws.load(SeqCst), 2);

    graphics.request_redraw(PhysicalSize::new(0, 0));
    futures_timer::Delay::new(Duration::from_millis(40)).await;
    assert_eq!(
        counts.draws.load(SeqCst),
        2,
        "zero-size output must not draw"
    );

    graphics.request_redraw(PhysicalSize::new(320, 240));
    wait_until(|| counts.draws.load(SeqCst) == 3).await;
    assert_eq!(
        counts.decodes.load(SeqCst),
        1,
        "resize must reuse the image"
    );
    input.unbounded_send(true).unwrap();
    wait_until(|| counts.updates.load(SeqCst) == 22).await;
    futures_timer::Delay::new(Duration::from_millis(40)).await;
    assert_eq!(
        counts.draws.load(SeqCst),
        3,
        "no demand means no more drawing"
    );

    // Continuously ready source work must still allow redraw and browser tasks.
    counts.flooding.store(true, SeqCst);
    input.unbounded_send(false).unwrap();
    wait_until(|| counts.updates.load(SeqCst) >= 50).await;
    wait_until(|| counts.callback_completed.load(SeqCst)).await;
    graphics.request_redraw(ready.size);
    wait_until(|| counts.draws.load(SeqCst) == 4).await;
    counts.flooding.store(false, SeqCst);

    drop(input);
    wait_until(|| counts.source_closed.load(SeqCst)).await;
    graphics.request_redraw(ready.size);
    wait_until(|| counts.draws.load(SeqCst) == 5).await;
    #[cfg(target_os = "linux")]
    assert!(window::native::presented_pixels_match(
        ready.window_id,
        ready.size
    ));
    graphics.request_close();
    graphics.request_close();
    graphics.finished().await.unwrap();
    drop(graphics);
    assert!(
        counts.dropped.load(SeqCst),
        "display must be released on graphics before shutdown completes"
    );
    close_window.await;
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires X11 and a graphics adapter; run under Xvfb"]
fn graphics_host_updates_draws_and_shuts_down_independently() {
    pollster::block_on(exercise_graphics());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn graphics_host_updates_draws_and_shuts_down_independently() {
    exercise_graphics().await;
}
