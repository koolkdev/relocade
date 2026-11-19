#![cfg(target_os = "linux")]

use std::{
    error::Error,
    process::{Child, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{
            AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask,
            ImageFormat, InputFocus, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, KEY_PRESS_EVENT,
            KEY_RELEASE_EVENT,
        },
        xtest::ConnectionExt as _,
    },
    CURRENT_TIME,
};

struct App(Child);
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires an X11 display and a graphics adapter"]
fn mouse_capture_keyboard_echo_resize_and_shutdown() -> Result<(), Box<dyn Error>> {
    let (x, screen) = x11rb::connect(None)?;
    let display = &x.setup().roots[screen];
    let root = display.root;
    let outside = (
        display.width_in_pixels as i16 - 1,
        display.height_in_pixels as i16 - 1,
    );
    let atom = |name: &[u8]| -> Result<u32, Box<dyn Error>> {
        Ok(x.intern_atom(false, name)?.reply()?.atom)
    };
    let pid_atom = atom(b"_NET_WM_PID")?;
    let protocols = atom(b"WM_PROTOCOLS")?;
    let delete = atom(b"WM_DELETE_WINDOW")?;
    let mut app = App(Command::new(env!("CARGO_BIN_EXE_relocade-dos"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?);
    let deadline = Instant::now() + Duration::from_secs(120);
    let window = loop {
        assert!(Instant::now() < deadline, "window did not appear");
        assert!(app.0.try_wait()?.is_none(), "app exited early");
        let found = x
            .query_tree(root)?
            .reply()?
            .children
            .into_iter()
            .find(|window| {
                x.get_property(false, *window, pid_atom, AtomEnum::CARDINAL, 0, 1)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .and_then(|reply| reply.value32().and_then(|mut values| values.next()))
                    == Some(app.0.id())
            });
        if let Some(window) = found {
            break window;
        }
        sleep(Duration::from_millis(20));
    };

    let resize = |width: u32, height: u32| -> Result<(), Box<dyn Error>> {
        x.configure_window(
            window,
            &ConfigureWindowAux::new().width(width).height(height),
        )?;
        x.flush()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let geometry = x.get_geometry(window)?.reply()?;
            if u32::from(geometry.width) == width && u32::from(geometry.height) == height {
                return Ok(());
            }
            assert!(Instant::now() < deadline, "resize did not arrive");
            sleep(Duration::from_millis(20));
        }
    };
    // Preserve every source dot when sampling the 720-pixel-wide startup image.
    resize(800, 600)?;

    // Literal sixteen-row firmware glyphs, sampled from the displayed window.
    let prompt_row = 6;
    let prompt = [0, 0, 0, 96, 48, 24, 12, 6, 12, 24, 48, 96, 0, 0, 0, 0];
    let x_glyph = [0, 0, 0, 0, 0, 198, 198, 108, 56, 108, 198, 198, 0, 0, 0, 0];
    let displayed_glyph = |row: usize, expected: [u8; 16]| -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let geometry = x.get_geometry(window)?.reply()?;
            let pixels = x
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    window,
                    0,
                    0,
                    geometry.width,
                    geometry.height,
                    u32::MAX,
                )?
                .reply()?;
            // Xvfb's 24-bit TrueColor output uses four BGR bytes per pixel.
            assert_eq!(
                pixels.data.len(),
                usize::from(geometry.width) * usize::from(geometry.height) * 4
            );
            let width = f64::from(geometry.width).min(f64::from(geometry.height) * 4.0 / 3.0);
            let height = width * 3.0 / 4.0;
            let left = (f64::from(geometry.width) - width) / 2.0;
            let top = (f64::from(geometry.height) - height) / 2.0;
            let actual: [u8; 16] = std::array::from_fn(|scan| {
                let mut bits = 0;
                for bit in 0..8 {
                    let x = (left + (bit as f64 + 0.5) * width / 720.0) as usize;
                    let y = (top + ((row * 16 + scan) as f64 + 0.5) * height / 400.0) as usize;
                    if pixels.data[(y * usize::from(geometry.width) + x) * 4] > 100 {
                        bits |= 0x80 >> bit;
                    }
                }
                bits
            });
            if actual == expected {
                return Ok(());
            }
            assert!(
                Instant::now() < deadline,
                "row {row}: {actual:?} != {expected:?}"
            );
            sleep(Duration::from_millis(30));
        }
    };
    displayed_glyph(prompt_row, prompt)?;
    x.set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;
    let first = x.setup().min_keycode;
    let keys = x
        .get_keyboard_mapping(first, x.setup().max_keycode - first + 1)?
        .reply()?;
    let type_keys = |bytes: &[u8]| -> Result<(), Box<dyn Error>> {
        for &byte in bytes {
            let symbol = match byte {
                8 => 0xff08,
                27 => 0xff1b,
                b'\r' => 0xff0d,
                _ => u32::from(byte),
            };
            let key = keys
                .keysyms
                .chunks(usize::from(keys.keysyms_per_keycode))
                .position(|symbols| symbols[0] == symbol)
                .expect("fixture key is in the X11 layout");
            for event in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
                x.xtest_fake_input(event, first + key as u8, CURRENT_TIME, root, 0, 0, 0)?;
            }
        }
        x.flush()?;
        Ok(())
    };
    let captured = |expected: bool| -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // X11 confinement clips a warp outside the captured window.
            x.warp_pointer(0u32, root, 0, 0, 0, 0, outside.0, outside.1)?;
            let pointer = x.query_pointer(root)?.reply()?;
            if ((pointer.root_x, pointer.root_y) != outside) == expected {
                return Ok(());
            }
            assert!(
                Instant::now() < deadline,
                "mouse capture did not become {expected}"
            );
            sleep(Duration::from_millis(20));
        }
    };
    let capture = || -> Result<(), Box<dyn Error>> {
        let origin = x.translate_coordinates(window, root, 0, 0)?.reply()?;
        let size = x.get_geometry(window)?.reply()?;
        assert!(
            i32::from(outside.0) < i32::from(origin.dst_x)
                || i32::from(outside.1) < i32::from(origin.dst_y)
                || i32::from(outside.0) >= i32::from(origin.dst_x) + i32::from(size.width)
                || i32::from(outside.1) >= i32::from(origin.dst_y) + i32::from(size.height),
            "capture fixture needs a display corner outside the application window"
        );
        x.warp_pointer(0u32, window, 0, 0, 0, 0, 100, 100)?;
        for event in [BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT] {
            x.xtest_fake_input(event, 1, CURRENT_TIME, root, 0, 0, 0)?;
        }
        x.flush()?;
        captured(true)
    };
    capture()?;
    type_keys(&[27])?;
    captured(false)?;
    capture()?;
    x.set_input_focus(InputFocus::PARENT, root, CURRENT_TIME)?
        .check()?;
    captured(false)?;
    x.set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?
        .check()?;
    type_keys(b"echo xy\x08\r")?;
    displayed_glyph(prompt_row + 1, x_glyph)?;
    displayed_glyph(prompt_row + 2, prompt)?;
    resize(960, 720)?;
    displayed_glyph(prompt_row + 1, x_glyph)?;
    type_keys(b"echo x\r")?;
    displayed_glyph(prompt_row + 3, x_glyph)?;
    displayed_glyph(prompt_row + 4, prompt)?;
    capture()?;
    x.send_event(
        false,
        window,
        EventMask::NO_EVENT,
        ClientMessageEvent::new(32, window, protocols, [delete, 0, 0, 0, 0]),
    )?;
    x.flush()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.0.try_wait()?.is_none() {
        assert!(Instant::now() < deadline, "app did not finish cleanup");
        sleep(Duration::from_millis(20));
    }
    captured(false)?;
    let errors = std::io::read_to_string(app.0.stderr.take().unwrap())?;
    assert!(app.0.wait()?.success(), "{errors}");
    Ok(())
}
