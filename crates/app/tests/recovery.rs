#![cfg(any(target_os = "linux", target_arch = "wasm32"))]

use futures_channel::mpsc;
use relocade_app::{graphics::Graphics, winit::dpi::PhysicalSize};
use relocade_peripherals::display::DisplayRenderer;
use relocade_renderer::Image;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        Arc, Mutex,
    },
    time::Duration,
};

#[path = "support/window.rs"]
mod window;

#[derive(Default)]
struct Source {
    connections: AtomicUsize,
    bindings: AtomicUsize,
    losses: AtomicUsize,
    guest_resets: AtomicUsize,
    images: AtomicUsize,
    draws: AtomicUsize,
    drops: AtomicUsize,
    release_rebuild: AtomicBool,
    colored: AtomicBool,
    owner: Mutex<Option<std::thread::ThreadId>>,
    #[cfg(target_arch = "wasm32")]
    acquisition_failed: AtomicBool,
}

enum Command {
    LoseDevice,
    Reset,
    #[cfg(target_arch = "wasm32")]
    Acquisition,
}

// Exercise acquisition loss without a matching device-loss callback. This must
// not depend on how many browser tasks pass before a callback might arrive.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function failAcquisitions(count) {
    const acquire = GPUCanvasContext.prototype.getCurrentTexture;
    GPUCanvasContext.prototype.getCurrentTexture = function() {
        if (--count === 0) GPUCanvasContext.prototype.getCurrentTexture = acquire;
        throw new Error('fixture surface loss');
    };
}
")]
extern "C" {
    fn failAcquisitions(count: u32);
}

struct Gpu {
    device: wgpu::Device,
    image: Option<Image>,
}

struct TestDisplay {
    gpu: Option<Gpu>,
    source: Arc<Source>,
    commands: mpsc::UnboundedReceiver<Command>,
    guest_lost: bool,
    updates_closed: bool,
}

fn upload_image(device: &wgpu::Device, queue: &wgpu::Queue, colored: bool) -> Image {
    let pixels = if colored {
        [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ]
    } else {
        [0; 16]
    };
    let image = Image::new(device, [2, 2], (1, 1)).unwrap();
    image.upload(queue, &pixels).unwrap();
    image
}

impl DisplayRenderer for TestDisplay {
    fn device_lost(&mut self) {
        self.gpu = None;
        self.guest_lost = true;
        self.source.losses.fetch_add(1, SeqCst);
    }

    fn attach_device(&mut self, device: &wgpu::Device, _queue: &wgpu::Queue) -> Result<(), String> {
        assert!(self.guest_lost && self.gpu.is_none());
        assert_eq!(
            self.source.drops.load(SeqCst),
            0,
            "connection must survive loss"
        );
        let generation = self.source.bindings.fetch_add(1, SeqCst) + 1;
        #[cfg(target_os = "linux")]
        if generation == 3 {
            return Err("fixture cannot attach replacement device".into());
        }
        assert!(
            generation <= 3,
            "loss before presentation must not loop retries"
        );
        if generation == 2 {
            // Main and the source stay usable during a slow GPU rebuild.
            while !self.source.release_rebuild.load(SeqCst) {
                std::hint::spin_loop();
            }
        }
        self.gpu = Some(Gpu {
            device: device.clone(),
            image: None,
        });
        // Host GPU availability must not clear the guest's lost-device state.
        // Updates keep processing commands while encode_image returns no image.
        Ok(())
    }

    async fn update(&mut self, queue: &wgpu::Queue) -> Option<bool> {
        assert!(
            !self.updates_closed,
            "recovery must not poll a closed source again"
        );
        match self.commands.recv().await.ok()? {
            Command::LoseDevice => {
                let device = &self.gpu.as_ref().unwrap().device;
                // Real loss callback: native dispatches through poll; the browser
                // resolves GPUDevice.lost on the graphics worker event loop.
                device.destroy();
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                std::future::pending().await
            }
            Command::Reset => {
                assert!(self.guest_lost);
                let gpu = self.gpu.as_mut().unwrap();
                gpu.image = Some(upload_image(
                    &gpu.device,
                    queue,
                    self.source.colored.load(SeqCst),
                ));
                self.guest_lost = false;
                self.source.guest_resets.fetch_add(1, SeqCst);
                Some(true)
            }
            #[cfg(target_arch = "wasm32")]
            Command::Acquisition => {
                failAcquisitions(2);
                self.source.acquisition_failed.store(true, SeqCst);
                self.updates_closed = true;
                None
            }
        }
    }

    fn encode_image(&mut self, _encoder: &mut wgpu::CommandEncoder) -> Option<&Image> {
        self.source.draws.fetch_add(1, SeqCst);
        let image = self.gpu.as_ref().unwrap().image.as_ref();
        if self.guest_lost {
            assert!(image.is_none());
        }
        if image.is_some() {
            self.source.images.fetch_add(1, SeqCst);
        }
        image
    }
}

impl Drop for TestDisplay {
    fn drop(&mut self) {
        assert_eq!(
            *self.source.owner.lock().unwrap(),
            Some(std::thread::current().id()),
        );
        self.source.drops.fetch_add(1, SeqCst);
    }
}

async fn wait_until(ready: impl Fn() -> bool) {
    for _ in 0..2000 {
        if ready() {
            return;
        }
        futures_timer::Delay::new(Duration::from_millis(2)).await;
    }
    panic!("graphics recovery timed out");
}

async fn exercise_recovery() {
    let (ready, close_window) = window::TestWindow::open().await.split();
    let source = Arc::new(Source::default());
    let retained = source.clone();
    let (commands, incoming) = mpsc::unbounded();
    let (mut graphics, finished) = Graphics::spawn(
        ready.setup,
        move |device, queue| {
            *retained.owner.lock().unwrap() = Some(std::thread::current().id());
            let image = upload_image(device, queue, false);
            retained.connections.fetch_add(1, SeqCst);
            retained.bindings.store(1, SeqCst);
            Ok(TestDisplay {
                gpu: Some(Gpu {
                    device: device.clone(),
                    image: Some(image),
                }),
                source: retained,
                commands: incoming,
                guest_lost: false,
                updates_closed: false,
            })
        },
        || {},
    );
    let exercise = async {
        wait_until(|| source.bindings.load(SeqCst) == 1).await;
        graphics.request_redraw(ready.size);
        wait_until(|| source.draws.load(SeqCst) == 1).await;
        commands.unbounded_send(Command::LoseDevice).unwrap();
        wait_until(|| source.bindings.load(SeqCst) == 2).await;
        source.colored.store(true, SeqCst);
        for width in 1..=1000 {
            graphics.request_redraw(PhysicalSize::new(width, 200));
        }
        graphics.request_redraw(ready.size);
        source.release_rebuild.store(true, SeqCst);
        wait_until(|| source.draws.load(SeqCst) == 2).await;
        futures_timer::Delay::new(Duration::from_millis(40)).await;
        assert_eq!(
            source.draws.load(SeqCst),
            2,
            "rebuild retains one pending draw"
        );
        assert_eq!(
            source.images.load(SeqCst),
            1,
            "replacement must not restore guest output"
        );
        assert_eq!(source.losses.load(SeqCst), 1);
        assert_eq!(source.guest_resets.load(SeqCst), 0);
        commands.unbounded_send(Command::Reset).unwrap();
        wait_until(|| source.guest_resets.load(SeqCst) == 1).await;
        graphics.request_redraw(ready.size);
        wait_until(|| source.images.load(SeqCst) == 2).await;
        #[cfg(target_os = "linux")]
        assert!(window::native::presented_pixels_match(
            ready.window_id,
            ready.size
        ));

        // A later loss can recover again. Native checks rebinding failure; web
        // checks acquisition loss before any callback and an unusable replacement.
        // Neither path should restart the worker or spin through more retries.
        #[cfg(target_os = "linux")]
        commands.unbounded_send(Command::LoseDevice).unwrap();
        #[cfg(target_arch = "wasm32")]
        {
            commands.unbounded_send(Command::Acquisition).unwrap();
            wait_until(|| source.acquisition_failed.load(SeqCst)).await;
            graphics.request_redraw(ready.size);
        }
        wait_until(|| source.bindings.load(SeqCst) == 3).await;
        #[cfg(target_arch = "wasm32")]
        {
            // Leave no redraw ready: reopening updates after replacement must
            // reach the terminal-source assertion instead of racing another loss.
            futures_timer::Delay::new(Duration::from_millis(40)).await;
            graphics.request_redraw(ready.size);
        }
        Ok::<(), String>(())
    };
    let completion = async {
        let error = finished.await.unwrap_err();
        #[cfg(target_os = "linux")]
        assert_eq!(error, "fixture cannot attach replacement device");
        #[cfg(target_arch = "wasm32")]
        assert_eq!(
            error,
            "Graphics recovery failed before presentation: Window surface was lost"
        );
        Ok::<(), String>(())
    };
    futures_lite::future::try_zip(exercise, completion)
        .await
        .unwrap();
    assert_eq!(source.connections.load(SeqCst), 1);
    assert_eq!(source.drops.load(SeqCst), 1);
    #[cfg(target_os = "linux")]
    assert_eq!(source.losses.load(SeqCst), 2);
    #[cfg(target_arch = "wasm32")]
    assert_eq!(source.losses.load(SeqCst), 3);
    drop(graphics);
    close_window.await;
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires X11 and a graphics adapter; run under Xvfb"]
fn device_replacement_preserves_connection_and_waits_for_backend_recovery() {
    pollster::block_on(exercise_recovery());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_replacement_preserves_connection_and_waits_for_backend_recovery() {
    exercise_recovery().await;
}
