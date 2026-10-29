#![cfg(target_os = "linux")]

use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, InputFocus,
        MapState, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
    },
};

struct ChildApp(Child);
impl Drop for ChildApp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "window event timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires an X11 display, run under Xvfb"]
fn native_input_resize_and_close_reach_the_main_thread_application() {
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
    let name = atom(b"_NET_WM_NAME");
    let utf8 = atom(b"UTF8_STRING");
    let protocols = atom(b"WM_PROTOCOLS");
    let close = atom(b"WM_DELETE_WINDOW");
    let mut child = ChildApp(
        Command::new(env!("CARGO_BIN_EXE_relocade-window-example"))
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
                property.value32().and_then(|mut values| values.next()) == Some(child.0.id())
                    && connection
                        .get_window_attributes(window)
                        .unwrap()
                        .reply()
                        .unwrap()
                        .map_state
                        == MapState::VIEWABLE
            })
    });
    let title = || {
        String::from_utf8(
            connection
                .get_property(false, window, name, utf8, 0, 1024)
                .unwrap()
                .reply()
                .unwrap()
                .value,
        )
        .unwrap()
    };
    wait(|| title().starts_with("Window 640x480").then_some(()));
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
        .position(|keys| keys.contains(&u32::from(b'y')))
        .unwrap();
    for event in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
        x11rb::protocol::xtest::fake_input(
            &connection,
            event,
            first + offset as u8,
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
    wait(|| title().contains("key=y").then_some(()));
    connection
        .configure_window(window, &ConfigureWindowAux::new().width(512).height(320))
        .unwrap()
        .check()
        .unwrap();
    connection.flush().unwrap();
    wait(|| title().starts_with("Window 512x320 | key=y").then_some(()));
    let observed_redraws: u32 = title().split_once("redraws=").unwrap().1.parse().unwrap();
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
    assert!(wait(|| child.0.try_wait().unwrap()).success());
    let mut output = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    let retained_redraws: u32 = output
        .trim()
        .strip_prefix("Window closed after ")
        .unwrap()
        .strip_suffix(" redraws")
        .unwrap()
        .parse()
        .unwrap();
    assert!(retained_redraws >= observed_redraws);
}
