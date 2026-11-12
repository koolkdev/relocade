use super::*;
use relocade_machine::IoBus;
use std::rc::Weak;

struct Startup {
    bus: Weak<IoBus>,
    value: u32,
    started: bool,
}

impl Device for Startup {
    async fn run(device: Rc<RefCell<Self>>) {
        let io = device.borrow().bus.upgrade().unwrap();
        // Both the port entry and the retained MMIO device must be
        // installed, with the setup runtime borrow released before this poll.
        assert_eq!(io.read_port(0x80, 1), 0x11);
        assert_eq!(io.read_mmio(0x2000, 1), 0x11);
        device.borrow_mut().started = true;
    }

    fn read_mmio(&mut self, _: IoContext<'_>, _: u32, _: u8) -> u64 {
        self.value.into()
    }
}

struct Adjacent;
impl Device for Adjacent {
    fn read_mmio(&mut self, _: IoContext<'_>, _: u32, _: u8) -> u64 {
        0x33
    }
}

async fn connected_before_startup() {
    let clock = ManualClock::default();
    let config = configuration();
    let mut machine = relocade_machine::Machine::new(config.machine).unwrap();
    let prepared = machine.create_cpu(config.cpu, None).unwrap();
    let worker = prepared.spawn_with_clock(
        move || clock.clock(),
        async |runner| {
            let io = Rc::new(runner.io());
            let device = Rc::new(RefCell::new(Startup {
                bus: Rc::downgrade(&io),
                value: 0x11,
                started: false,
            }));
            runner
                .connect(device.clone(), |io| {
                    io.map_mmio(std::iter::once(0x2000..=0x2000));
                    let callbacks = relocade_machine::DeviceCallbacks::new(&device);
                    let read = callbacks.read(|_, device| device.value);
                    let write = callbacks.write(|_, device, value| device.value = value);
                    io.bind(
                        0x80..=0x81,
                        &relocade_machine::PortRead::new(move |context, _, _| read(context)),
                        &relocade_machine::PortWrite::new(move |context, _, _, value| {
                            write(context, value)
                        }),
                    );
                })
                .await;
            assert!(device.borrow().started);
            io.write_port(0x81, 1, 0x22);
            assert_eq!(io.read_port(0x80, 1), 0x22);

            runner
                .connect(Rc::new(RefCell::new(Adjacent)), |io| {
                    io.map_mmio(std::iter::once(0x2001..=0x2001));
                    io.bind(
                        0x82,
                        &relocade_machine::PortRead::new(|_, _, _| 0x44),
                        &relocade_machine::PortWrite::new(|_, _, _, _| {}),
                    );
                })
                .await;
            assert_eq!(io.read_mmio(0x2000, 2), 0x3322);
            assert_eq!(io.read_port(0x82, 1), 0x44);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn connection_installs_mappings_before_activity_and_preserves_previous_devices() {
    pollster::block_on(connected_before_startup());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn connection_installs_mappings_before_activity_and_preserves_previous_devices() {
    connected_before_startup().await;
}
