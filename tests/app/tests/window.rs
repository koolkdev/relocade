#![cfg(target_os = "linux")]

use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, ImageFormat,
        ImageOrder, InputFocus, MapState, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
    },
};

struct ChildApp(Child);
impl Drop for ChildApp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait<T>(mut condition: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = condition() {
            return value;
        }
        assert!(Instant::now() < deadline, "application window timed out");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires an X11 display, run under Xvfb"]
fn displays_image_and_closes_workers_before_exit() {
    exercise_window(false);
}

#[test]
#[ignore = "requires an X11 display, run under Xvfb"]
fn dos_accepts_native_input_and_retains_its_image_across_resize() {
    exercise_window(true);
}

fn exercise_window(dos: bool) {
    // Xvfb has no compositor; overlapping test windows obscure displayed pixels.
    static DISPLAY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _display = DISPLAY.lock().unwrap();
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let atom = |name: &[u8]| {
        connection
            .intern_atom(false, name)
            .unwrap()
            .reply()
            .unwrap()
            .atom
    };
    let pid = atom(b"_NET_WM_PID");
    let protocols = atom(b"WM_PROTOCOLS");
    let close = atom(b"WM_DELETE_WINDOW");
    let mut app = ChildApp(
        Command::new(env!("CARGO_BIN_EXE_relocade-app-tests"))
            .args(if dos { vec!["dos"] } else { vec![] })
            .env("WINIT_UNIX_BACKEND", "x11")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let window = wait(|| {
        connection
            .query_tree(root)
            .unwrap()
            .reply()
            .unwrap()
            .children
            .into_iter()
            .find(|&window| {
                let property = connection
                    .get_property(false, window, pid, AtomEnum::CARDINAL, 0, 1)
                    .unwrap()
                    .reply()
                    .unwrap();
                property.value32().and_then(|mut values| values.next()) == Some(app.0.id())
                    && connection
                        .get_window_attributes(window)
                        .unwrap()
                        .reply()
                        .unwrap()
                        .map_state
                        == MapState::VIEWABLE
            })
    });
    let pixel = |x, y| {
        let image = connection
            .get_image(ImageFormat::Z_PIXMAP, window, x, y, 1, 1, u32::MAX)
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(image.depth, 24);
        let bytes = image.data[..4].try_into().unwrap();
        let value = if connection.setup().image_byte_order == ImageOrder::LSB_FIRST {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        };
        [(value >> 16) as u8, (value >> 8) as u8, value as u8]
    };
    if dos {
        wait(|| (pixel(4, 4) == [170, 0, 0]).then_some(()));
        connection
            .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let first = connection.setup().min_keycode;
        let mapping = connection
            .get_keyboard_mapping(first, connection.setup().max_keycode - first + 1)
            .unwrap()
            .reply()
            .unwrap();
        let offset = mapping
            .keysyms
            .chunks(mapping.keysyms_per_keycode.into())
            .position(|codes| codes.contains(&u32::from(b'y')))
            .unwrap();
        let code = first + offset as u8;
        for event in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
            x11rb::protocol::xtest::fake_input(
                &connection,
                event,
                code,
                x11rb::CURRENT_TIME,
                root,
                0,
                0,
                0,
            )
            .unwrap()
            .check()
            .unwrap();
        }
        connection.flush().unwrap();
        wait(|| (pixel(4, 4) == [0, 170, 0]).then_some(()));
        connection
            .configure_window(window, &ConfigureWindowAux::new().width(800).height(600))
            .unwrap()
            .check()
            .unwrap();
        connection.flush().unwrap();
        wait(|| {
            let geometry = connection.get_geometry(window).unwrap().reply().unwrap();
            (geometry.width == 800 && geometry.height == 600 && pixel(4, 4) == [0, 170, 0])
                .then_some(())
        });
    } else {
        wait(|| {
            (pixel(300, 180) == [170, 0, 0] && pixel(660, 540) == [255, 255, 85]).then_some(())
        });
    }
    connection
        .send_event(
            false,
            window,
            EventMask::NO_EVENT,
            ClientMessageEvent::new(32, window, protocols, [close, 0, 0, 0, 0]),
        )
        .unwrap()
        .check()
        .unwrap();
    connection.flush().unwrap();
    let status = wait(|| app.0.try_wait().unwrap());
    assert!(status.success());
    use std::io::Read;
    let mut output = String::new();
    app.0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(
        output.contains(if dos { "DOS closed" } else { "Some(7)" }),
        "{output}"
    );
}
