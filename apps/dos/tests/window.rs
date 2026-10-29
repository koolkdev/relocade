#![cfg(target_os = "linux")]

use std::{
    process::{Child, Command},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{self, ConnectionExt, ImageFormat, ImageOrder},
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
fn program_exit_retains_display_through_resize_and_window_close() {
    let program = std::env::temp_dir().join(format!("relocade-window-{}.com", std::process::id()));
    // Write a white A on red through guest VRAM, then exit with code 7.
    std::fs::write(
        &program,
        [
            0xb8, 0x00, 0xb8, 0x8e, 0xc0, 0x26, 0xc7, 0x06, 0x00, 0x00, 0x41, 0x4f, 0xb8, 0x07,
            0x4c, 0xcd, 0x21,
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
    let window = until("program exit", || {
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
                    .is_ok_and(|reply| String::from_utf8_lossy(&reply.value).contains("exited 7"))
            })
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
