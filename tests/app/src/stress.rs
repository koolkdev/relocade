//! Test-only gates around the real DOS application; no alternate execution loop.
use relocade_app::{Application, Context, Input, WindowApplication};
use relocade_dos_app::DosApplication;
use relocade_vga::FrameSource;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering},
    OnceLock,
};

#[derive(Default)]
struct Observations {
    frames: OnceLock<FrameSource>,
    hold: AtomicBool,
    held: AtomicBool,
    draws: AtomicU32,
    width: AtomicU32,
    height: AtomicU32,
    dropped: AtomicU8,
    destroy_device: AtomicBool,
    check_next_size: AtomicBool,
    next_width: AtomicU32,
    next_height: AtomicU32,
}
static OBSERVATIONS: OnceLock<Observations> = OnceLock::new();
fn observations() -> &'static Observations {
    OBSERVATIONS.get().expect("fixture started on main")
}

pub struct StressDos(DosApplication);
impl Default for StressDos {
    fn default() -> Self {
        OBSERVATIONS
            .set(Observations::default())
            .ok()
            .expect("one fixture per window process");
        Self(DosApplication::new(vec![
            0xb8, 0x00, 0xb8, 0x8e, 0xc0, // ES = B800
            0x26, 0xc7, 0x06, 0, 0, 0xdb, 0x44, // red cell
            0xba, 0xda, 0x03, 0xec, // read VGA status
            0x26, 0xff, 0x06, 2, 0, // increment a VRAM counter
            0xb4, 0x06, 0xb2, 0xff, 0xcd, 0x21, // poll DOS input
            0x74, 0xef, 0x3c, b'y', 0x75, 0xeb, // loop until y
            0x26, 0xc7, 0x06, 0, 0, 0xdb, 0x22, // green cell
            0xb8, 0, 0x4c, 0xcd, 0x21,
        ]))
    }
}
struct ExecutionDrop;
impl Drop for ExecutionDrop {
    fn drop(&mut self) {
        observations().dropped.fetch_or(1, Ordering::Release);
    }
}
pub struct Graphics {
    inner: relocade_dos_app::Graphics,
    device: wgpu::Device,
}
impl Drop for Graphics {
    fn drop(&mut self) {
        observations().dropped.fetch_or(2, Ordering::Release);
    }
}
impl Application for StressDos {
    type Input = Input;
    type Source = FrameSource;
    type Output = ();
    async fn run(self, context: Context<Input, FrameSource>) -> Result<(), String> {
        let _drop = ExecutionDrop;
        self.0.run(context).await
    }
}
impl WindowApplication for StressDos {
    type Graphics = Graphics;

    fn create_graphics(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<Graphics, String> {
        Ok(Graphics {
            inner: DosApplication::create_graphics(device.clone(), queue, format)?,
            device,
        })
    }
    fn draw(
        graphics: &mut Graphics,
        source: &FrameSource,
        attachment: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), String> {
        let state = observations();
        state.frames.get_or_init(|| source.clone());
        if state.check_next_size.swap(false, Ordering::AcqRel) {
            state.next_height.store(size[1], Ordering::Release);
            // Width publishes the pair to the browser's waiting assertion.
            state.next_width.store(size[0], Ordering::Release);
        }
        if state.hold.load(Ordering::Acquire) {
            state.held.store(true, Ordering::Release);
            let deadline = web_time::Instant::now() + std::time::Duration::from_secs(30);
            while state.hold.load(Ordering::Acquire) {
                assert!(
                    web_time::Instant::now() < deadline,
                    "graphics gate was not released"
                );
                std::hint::spin_loop();
            }
            state.held.store(false, Ordering::Release);
            state.check_next_size.store(true, Ordering::Release);
        }
        if state.destroy_device.swap(false, Ordering::AcqRel) {
            graphics.device.destroy();
            return Ok(());
        }
        DosApplication::draw(&mut graphics.inner, source, attachment, size)?;
        state.width.store(size[0], Ordering::Release);
        state.height.store(size[1], Ordering::Release);
        state.draws.fetch_add(1, Ordering::Release);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start_stress() -> Result<relocade_app::web::BrowserControl, wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    relocade_app::open(StressDos::default(), Default::default())
        .map(|control| relocade_app::web::control(control, |()| wasm_bindgen::JsValue::UNDEFINED))
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hold_graphics(hold: bool) {
    observations().hold.store(hold, Ordering::Release);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn destroy_device() {
    observations().destroy_device.store(true, Ordering::Release);
}

/// Snapshot of test observations. Reading it never waits for the graphics owner.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn stress_state() -> wasm_bindgen::JsValue {
    let s = observations();
    let capture = s.frames.get().and_then(FrameSource::current_capture);
    let (sequence, counter, color) = capture.map_or((0, 0, 0), |c| {
        (
            c.sequence as u32,
            u32::from(c.screen.cells[1].character) | u32::from(c.screen.cells[1].attribute) << 8,
            u32::from(c.screen.cells[0].attribute),
        )
    });
    let result = js_sys::Object::new();
    for (name, value) in [
        ("held", u32::from(s.held.load(Ordering::Acquire))),
        ("draws", s.draws.load(Ordering::Acquire)),
        ("width", s.width.load(Ordering::Acquire)),
        ("height", s.height.load(Ordering::Acquire)),
        ("sequence", sequence),
        ("counter", counter),
        ("color", color),
        ("dropped", u32::from(s.dropped.load(Ordering::Acquire))),
        ("nextWidth", s.next_width.load(Ordering::Acquire)),
        ("nextHeight", s.next_height.load(Ordering::Acquire)),
    ] {
        js_sys::Reflect::set(&result, &name.into(), &value.into()).unwrap();
    }
    result.into()
}
