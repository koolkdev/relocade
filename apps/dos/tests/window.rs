// Winit requires main and one native event loop per process.
// CI opts in under Xvfb; each supplied-byte fixture uses a separate process.
fn main() {
    #[cfg(target_os = "linux")]
    {
        let args: Vec<_> = std::env::args().collect();
        if let Some(index) = args.iter().position(|arg| arg == "--fixture") {
            let running = match args.get(index + 1).map(String::as_str) {
                Some("running") => true,
                Some("exited") => false,
                _ => panic!("unknown window fixture"),
            };
            std::process::exit(i32::from(linux::run_fixture(running)));
        } else if args.iter().any(|arg| arg == "--ignored") {
            linux::check();
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        io::Read,
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    use x11rb::{
        connection::Connection,
        protocol::xproto::{
            Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask,
            ImageFormat, ImageOrder, MapState, Window,
        },
    };

    const TIMEOUT: Duration = Duration::from_secs(60);

    struct Guest {
        child: Child,
    }

    impl Guest {
        fn start(case: &str) -> Self {
            let child = Command::new(std::env::current_exe().unwrap())
                .arg("--fixture")
                .arg(case)
                .env("WINIT_UNIX_BACKEND", "x11")
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            Self { child }
        }

        fn stderr(&mut self) -> String {
            let mut output = String::new();
            if let Some(mut stderr) = self.child.stderr.take() {
                stderr.read_to_string(&mut output).unwrap();
            }
            output
        }

        fn wait<T>(&mut self, description: &str, mut check: impl FnMut() -> Option<T>) -> T {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                if let Some(result) = check() {
                    return result;
                }
                if let Some(status) = self.child.try_wait().unwrap() {
                    panic!(
                        "application exited before {description}: {status}\n{}",
                        self.stderr()
                    );
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {description}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn wait_exit(&mut self, code: i32) {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert_eq!(status.code(), Some(code), "{}", self.stderr());
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "application did not finish shutdown"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for Guest {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn atom(connection: &impl Connection, name: &[u8]) -> Atom {
        connection
            .intern_atom(false, name)
            .unwrap()
            .reply()
            .unwrap()
            .atom
    }

    fn image(running: bool) -> Vec<u8> {
        let mut image = vec![
            0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
            0x31, 0xff, 0xfc, // DI = 0; CLD.
            0xb8, b' ', 0x10, // Blue background.
            0xb9, 0xc0, 3, 0xf3, 0xab, // Fill 12 rows (960 cells).
            0xb8, b' ', 0x20, // Green background.
            0xb9, 0x10, 4, 0xf3, 0xab, // Fill the remaining 13 rows.
        ];
        if running {
            image.extend_from_slice(&[0xeb, 0xfe]); // Keep executing until the host closes.
        }
        image.extend_from_slice(&[0xb8, 7, 0x4c, 0xcd, 0x21]); // Exit 7.
        image
    }

    fn pixel(connection: &impl Connection, window: Window, x: i16, y: i16) -> [u8; 3] {
        let image = connection
            .get_image(ImageFormat::Z_PIXMAP, window, x, y, 1, 1, u32::MAX)
            .unwrap()
            .reply()
            .unwrap();
        // CI's Xvfb runs with a 24-bit TrueColor screen and 32-bit pixel storage.
        assert_eq!(image.depth, 24);
        let bytes = image.data[..4].try_into().unwrap();
        let value = if connection.setup().image_byte_order == ImageOrder::LSB_FIRST {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        };
        [(value >> 16) as u8, (value >> 8) as u8, value as u8]
    }

    pub fn check() {
        let (connection, screen) = x11rb::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let pid_atom = atom(&connection, b"_NET_WM_PID");
        let protocols = atom(&connection, b"WM_PROTOCOLS");
        let delete = atom(&connection, b"WM_DELETE_WINDOW");

        for running in [false, true] {
            let mut guest = Guest::start(if running { "running" } else { "exited" });
            let pid = guest.child.id();
            let window = guest.wait("the mapped application window", || {
                connection
                    .query_tree(root)
                    .unwrap()
                    .reply()
                    .unwrap()
                    .children
                    .into_iter()
                    .find(|&window| {
                        let property = connection
                            .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
                            .unwrap()
                            .reply()
                            .unwrap();
                        property.value32().and_then(|mut values| values.next()) == Some(pid)
                            && connection
                                .get_window_attributes(window)
                                .unwrap()
                                .reply()
                                .unwrap()
                                .map_state
                                == MapState::VIEWABLE
                    })
            });
            guest.wait("a presented VGA frame", || {
                (pixel(&connection, window, 100, 100) == [0, 0, 170]
                    && pixel(&connection, window, 100, 320) == [0, 0, 170]
                    && pixel(&connection, window, 100, 600) == [0, 170, 0])
                .then_some(())
            });
            connection
                .configure_window(window, &ConfigureWindowAux::new().width(800).height(600))
                .unwrap()
                .check()
                .unwrap();
            connection.flush().unwrap();
            guest.wait("a resized window with continued presentation", || {
                let geometry = connection.get_geometry(window).unwrap().reply().unwrap();
                (geometry.width == 800
                    && geometry.height == 600
                    && pixel(&connection, window, 100, 100) == [0, 0, 170]
                    && pixel(&connection, window, 100, 320) == [0, 170, 0]
                    && pixel(&connection, window, 100, 500) == [0, 170, 0])
                .then_some(())
            });
            connection
                .send_event(
                    false,
                    window,
                    EventMask::NO_EVENT,
                    ClientMessageEvent::new(32, window, protocols, [delete, 0, 0, 0, 0]),
                )
                .unwrap()
                .check()
                .unwrap();
            connection.flush().unwrap();
            guest.wait_exit(if running { 0 } else { 7 });
        }
    }

    pub fn run_fixture(running: bool) -> u8 {
        use relocade_app::Exit;
        match relocade_dos_app::DosApplication::new(image(running))
            .open()
            .and_then(pollster::block_on)
        {
            Ok(Exit::Completed(code)) => code,
            Ok(Exit::Cancelled) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        }
    }
}
