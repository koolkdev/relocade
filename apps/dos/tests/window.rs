#![cfg(target_os = "linux")]

use std::{
    process::{Child, Command},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{self, ConnectionExt, ImageFormat, ImageOrder, InputFocus},
        xtest::ConnectionExt as _,
    },
};

struct App(Child, std::path::PathBuf);
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_file(&self.1);
    }
}

fn until<T>(state: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "application did not reach {state}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires X11 and a graphics adapter; run under Xvfb"]
fn keyboard_input_resumes_guest_and_retains_display_after_exit() {
    let program = std::env::temp_dir().join(format!("relocade-window-{}.com", std::process::id()));
    // Await a DOS key, write a white A on red, then return the received character.
    std::fs::write(
        &program,
        [
            0xb4, 0x08, 0xcd, 0x21, 0x88, 0xc3, 0xb8, 0x00, 0xb8, 0x8e, 0xc0, 0x26, 0xc7, 0x06,
            0x00, 0x00, 0x41, 0x4f, 0x88, 0xd8, 0xb4, 0x4c, 0xcd, 0x21,
        ],
    )
    .unwrap();
    let mut app = App(
        Command::new(env!("CARGO_BIN_EXE_relocade-dos-app"))
            .arg(&program)
            .spawn()
            .unwrap(),
        program,
    );
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let title = connection
        .intern_atom(false, b"_NET_WM_NAME")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let utf8 = connection
        .intern_atom(false, b"UTF8_STRING")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let window = until("window creation", || {
        assert!(
            app.0.try_wait().unwrap().is_none(),
            "application exited before close"
        );
        connection
            .query_tree(root)
            .unwrap()
            .reply()
            .unwrap()
            .children
            .into_iter()
            .find(|&id| {
                connection
                    .get_property(false, id, title, utf8, 0, 100)
                    .unwrap()
                    .reply()
                    .is_ok_and(|reply| {
                        String::from_utf8_lossy(&reply.value).contains("Relocade DOS")
                    })
            })
    });
    connection
        .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
        .unwrap();
    let setup = connection.setup();
    let keys = connection
        .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
        .unwrap()
        .reply()
        .unwrap();
    let keycode = |symbol| {
        setup.min_keycode
            + keys
                .keysyms
                .chunks(usize::from(keys.keysyms_per_keycode))
                .position(|symbols| symbols[0] == symbol)
                .unwrap() as u8
    };
    for (event, key) in [
        (xproto::KEY_PRESS_EVENT, keycode(0xffe1)), // left Shift
        (xproto::KEY_PRESS_EVENT, keycode(u32::from(b'a'))),
        (xproto::KEY_RELEASE_EVENT, keycode(u32::from(b'a'))),
        (xproto::KEY_RELEASE_EVENT, keycode(0xffe1)),
    ] {
        connection
            .xtest_fake_input(event, key, x11rb::CURRENT_TIME, root, 0, 0, 0)
            .unwrap();
    }
    connection.flush().unwrap();
    until("DOS receiving uppercase A", || {
        connection
            .get_property(false, window, title, utf8, 0, 100)
            .unwrap()
            .reply()
            .is_ok_and(|reply| String::from_utf8_lossy(&reply.value).contains("exited 65"))
            .then_some(())
    });
    let pixel = || {
        let image = connection
            .get_image(ImageFormat::Z_PIXMAP, window, 0, 0, 1, 1, u32::MAX)
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
    until("red guest background", || {
        (pixel() == [170, 0, 0]).then_some(())
    });
    connection
        .configure_window(
            window,
            &xproto::ConfigureWindowAux::new().width(640).height(480),
        )
        .unwrap();
    connection.flush().unwrap();
    until("resized display", || {
        let geometry = connection.get_geometry(window).unwrap().reply().unwrap();
        (geometry.width == 640 && geometry.height == 480 && pixel() == [170, 0, 0]).then_some(())
    });
    let protocols = connection
        .intern_atom(false, b"WM_PROTOCOLS")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let delete = connection
        .intern_atom(false, b"WM_DELETE_WINDOW")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    connection
        .send_event(
            false,
            window,
            xproto::EventMask::NO_EVENT,
            xproto::ClientMessageEvent::new(32, window, protocols, [delete, 0, 0, 0, 0]),
        )
        .unwrap();
    connection.flush().unwrap();
    assert!(until("window shutdown", || app.0.try_wait().unwrap()).success());
}
