//! Output-only PC parallel-port registers and logical connector levels (SPP).
//!
//! The register layout follows the [TI TL16C452 data sheet](https://www.ti.com/lit/ds/symlink/tl16c452.pdf),
//! tables 7–8. Data and control writes drive the connector; status reads sample
//! its inputs. The port gates ACK onto its PIC IRQ. Connected devices own their
//! protocols, including any handshake and byte buffering.
//!
//! Pins are logical levels: `true` means high, and `_n` denotes active low.
//! Empty inputs are pulled high. Electrical timing, reverse data, EPP and ECP
//! are not modeled.
#![forbid(unsafe_code)]

use event_listener::Event;
use relocade_machine::{Device, DeviceCallbacks, DeviceConnection, PortRead, PortWrite, Runner};
use relocade_pic::IrqLine;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
};

/// Levels driven by the peripheral into the parallel port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputPins {
    pub busy: bool,
    pub acknowledge_n: bool,
    pub paper_end: bool,
    pub selected: bool,
    pub error_n: bool,
}

impl Default for InputPins {
    fn default() -> Self {
        Self {
            busy: true,
            acknowledge_n: true,
            paper_end: true,
            selected: true,
            error_n: true,
        }
    }
}

/// Levels driven by the PC onto the connector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputPins {
    pub data: u8,
    pub strobe_n: bool,
    pub auto_feed_n: bool,
    pub initialize_n: bool,
    pub select_in_n: bool,
}

struct InputState {
    pins: Cell<InputPins>,
    ready: Event,
    irq_enabled: Cell<bool>,
    irq: IrqLine,
}

/// A connected peripheral's input driver on the execution owner.
///
/// Retain a clone to change status and IRQ asynchronously. Driving inputs does
/// not borrow the port, so it is also safe inside an output notification.
#[derive(Clone)]
pub struct Inputs(Rc<InputState>);

impl Inputs {
    pub fn set(&self, pins: InputPins) {
        let previous = self.0.pins.replace(pins);
        self.update_irq();
        if previous.busy && !pins.busy {
            self.0.ready.notify(usize::MAX);
        }
    }

    fn update_irq(&self) {
        // Hardware gates the ACK level. The PIC owns masking and edge detection.
        self.0
            .irq
            .set_level(self.0.irq_enabled.get() && self.0.pins.get().acknowledge_n);
    }
}

type OutputHandler = Box<dyn FnMut(OutputPins, &Inputs)>;

/// Execution-owned SPP registers and one optional peripheral connection.
pub struct Port {
    base: u16,
    data: u8,
    control: u8,
    inputs: Inputs,
    peripheral: Option<OutputHandler>,
}

impl Port {
    /// Creates reset-state latches with pulled-up inputs and a disabled IRQ.
    /// The caller supplies the port's dedicated PIC line, initially low.
    pub fn new(base: u16, irq: IrqLine) -> Self {
        assert!(base <= u16::MAX - 2, "parallel-port register range");
        Self {
            base,
            data: 0,
            control: 0,
            inputs: Inputs(Rc::new(InputState {
                pins: Cell::new(InputPins::default()),
                ready: Event::new(),
                irq_enabled: Cell::new(false),
                irq,
            })),
            peripheral: None,
        }
    }

    pub async fn initialize(runner: &Runner, base: u16, irq: IrqLine) -> Rc<RefCell<Self>> {
        let port = Rc::new(RefCell::new(Self::new(base, irq)));
        runner
            .connect(port.clone(), |io| Self::configure_io(io, &port))
            .await;
        port
    }

    /// Base address of this adapter's data, status and control registers.
    pub fn base(&self) -> u16 {
        self.base
    }

    /// Waits for the peripheral to deassert BUSY, without transferring data.
    /// The owned future holds no port borrow and unregisters on cancellation.
    /// It completes immediately if ready; otherwise input changes wake it.
    pub fn wait_ready(&self) -> impl Future<Output = ()> + 'static + use<> {
        let inputs = self.inputs.0.clone();
        async move {
            loop {
                let ready = inputs.ready.listen();
                if !inputs.pins.get().busy {
                    return;
                }
                ready.await;
            }
        }
    }

    /// Attaches one peripheral while guest execution is stopped, immediately
    /// supplying the current outputs and its input driver. Later notifications
    /// report every output change synchronously, including same-timestamp edges.
    /// The callback must not block or reenter this port. Inputs may be driven
    /// immediately or retained for asynchronous activity. Attachment is permanent
    /// for this port's lifetime; attaching to an occupied port is an error.
    pub fn attach(&mut self, mut peripheral: impl FnMut(OutputPins, &Inputs) + 'static) {
        assert!(self.peripheral.is_none(), "parallel port is already occupied");
        peripheral(self.output_pins(), &self.inputs);
        self.peripheral = Some(Box::new(peripheral));
    }

    /// Binds data, read-only status and control at three consecutive addresses.
    pub fn configure_io(io: &mut DeviceConnection<'_>, port: &Rc<RefCell<Self>>) {
        let base = port.borrow().base;
        let callbacks = DeviceCallbacks::new(port);
        io.bind(
            base,
            &PortRead::byte(callbacks.read(|_, port| port.data)),
            &PortWrite::byte(callbacks.write(|_, port, value| {
                if port.data != value {
                    port.data = value;
                    port.notify();
                }
            })),
        );
        io.bind(
            base + 1,
            &PortRead::byte(callbacks.read(|_, port| port.status())),
            &PortWrite::byte(|_, _| {}),
        );
        io.bind(
            base + 2,
            &PortRead::byte(callbacks.read(|_, port| port.control | 0xe0)),
            &PortWrite::byte(callbacks.write(|_, port, value| {
                let previous = port.control;
                port.control = value & 0x1f;
                port.inputs.0.irq_enabled.set(port.control & 0x10 != 0);
                port.inputs.update_irq();
                if (previous ^ port.control) & 0x0f != 0 {
                    port.notify();
                }
            })),
        );
    }

    pub fn output_pins(&self) -> OutputPins {
        OutputPins {
            data: self.data,
            strobe_n: self.control & 1 == 0,
            auto_feed_n: self.control & 2 == 0,
            initialize_n: self.control & 4 != 0,
            select_in_n: self.control & 8 == 0,
        }
    }

    fn notify(&mut self) {
        let outputs = self.output_pins();
        if let Some(peripheral) = &mut self.peripheral {
            peripheral(outputs, &self.inputs);
        }
    }

    fn status(&self) -> u8 {
        let pins = self.inputs.0.pins.get();
        (u8::from(!pins.busy) << 7)
            | (u8::from(pins.acknowledge_n) << 6)
            | (u8::from(pins.paper_end) << 5)
            | (u8::from(pins.selected) << 4)
            | (u8::from(pins.error_n) << 3)
            | 7
    }
}

impl Device for Port {}
