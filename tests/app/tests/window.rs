#![cfg(target_os = "linux")]

use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, ImageFormat, ImageOrder, MapState,
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
    wait(|| (pixel(300, 180) == [170, 0, 0] && pixel(660, 540) == [255, 255, 85]).then_some(()));
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
    assert!(output.contains("Some(7)"), "{output}");
}
