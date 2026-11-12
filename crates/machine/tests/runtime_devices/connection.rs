use super::*;
use relocade_machine::{IoBus, MmioRead, MmioWrite};
use std::rc::Weak;

struct Startup {
    bus: Weak<IoBus>,
    value: u32,
    started: bool,
}

impl Device for Startup {
    async fn run(device: Rc<RefCell<Self>>) {
        let io = device.borrow().bus.upgrade().unwrap();
        // Both the port and MMIO operations must be
        // installed, with the setup runtime borrow released before this poll.
        assert_eq!(io.read_port(0x80, 1), 0x11);
        assert_eq!(io.read_mmio(0x2000, 1), 0x11);
        device.borrow_mut().started = true;
    }
}

struct Adjacent;
impl Device for Adjacent {}

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
                    let callbacks = relocade_machine::DeviceCallbacks::new(&device);
                    io.bind_mmio(
                        0x2000..=0x2000,
                        &MmioRead::new(callbacks.read_mmio(|_, device, _, _| device.value.into())),
                        &MmioWrite::new(|_, _, _, _| {}),
                    );
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
                    io.bind_mmio(
                        0x2001..=0x2001,
                        &MmioRead::new(|_, _, _| 0x33),
                        &MmioWrite::new(|_, _, _, _| {}),
                    );
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
