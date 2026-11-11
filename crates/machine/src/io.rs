//! Device routing and synchronous access on one execution worker.

mod connection;
mod context;
mod device;
pub(crate) mod ports;
use connection::ConnectedDevice;
pub use connection::DeviceConnection;
pub use context::IoContext;
pub use device::Device;
pub use ports::{PortBinding, PortRange, PortRead, PortWrite};

use crate::runtime::WasmRuntimeHandle;
use relocade_wasm_runtime::WasmBackend;
use std::{
    cell::{Cell, RefCell},
    ops::RangeInclusive,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

/// An owned handle to device connections and their runtime on one execution worker.
/// Obtain it through [`crate::Cpu::io`] or clone an existing handle. Clones share
/// routing and retain the devices and runtime independently of the CPU controller.
/// Port and MMIO methods perform synchronous transfers through `&self`. Retain
/// the handle across awaits and use it at stopped host boundaries, including
/// after the CPU is dropped. Engine callbacks hold weak routing references.
/// A bus can be attached to only one CPU over its lifetime, including after
/// that CPU is dropped or its initialization fails.
/// Unconnected reads return FF bytes and unconnected writes are ignored.
///
/// Host accesses briefly borrow the stopped runtime. Device callbacks use their
/// supplied [`IoContext`] instead of reacquiring it through this handle.
///
/// ```compile_fail
/// let io = relocade_machine::IoBus::default();
/// std::thread::spawn(move || io.read_port(0x3d4, 1));
/// ```
#[derive(Clone)]
pub struct IoBus {
    state: Rc<State>,
}

pub(crate) struct State {
    runtime: WasmRuntimeHandle,
    cpu_attached: Cell<bool>,
    connections: RefCell<Vec<ConnectedDevice>>,
    ports: ports::Ports,
    identity: Weak<State>,
}

impl Default for IoBus {
    fn default() -> Self {
        Self {
            state: Rc::new_cyclic(|identity| State {
                runtime: WasmRuntimeHandle::default(),
                cpu_attached: Cell::new(false),
                connections: RefCell::default(),
                ports: ports::Ports::default(),
                identity: identity.clone(),
            }),
        }
    }
}

impl IoBus {
    /// Creates a bus with one configured device. Further connections use
    /// [`Self::connect`] and share the same port tables.
    pub fn new(
        device: Rc<RefCell<dyn Device>>,
        configure: impl FnOnce(&mut DeviceConnection<'_>),
    ) -> Self {
        let io = Self::default();
        io.connect(device, configure);
        io
    }

    /// Attaches a device and configures its mappings on this bus. Existing devices
    /// and already-linked guest modules see the mappings immediately. Call this
    /// at a stopped host boundary; [`crate::Runner::connect`] also starts activity.
    /// The configuration context cannot escape the closure or cross an await.
    ///
    /// # Panics
    /// Panics if a range is empty or overlaps another mapping.
    pub fn connect(
        &self,
        device: Rc<RefCell<dyn Device>>,
        configure: impl FnOnce(&mut DeviceConnection<'_>),
    ) {
        self.with_context(|context| {
            let mut connection = DeviceConnection::new(context, device);
            configure(&mut connection);
            self.state
                .connections
                .borrow_mut()
                .push(connection.finish());
        });
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// Runs synchronous host I/O with a scoped runtime context. The context cannot
    /// escape this call or remain borrowed across an await. Reuse it when a batch
    /// performs several transfers. Panics if guest execution already owns the runtime.
    pub fn with_context<R>(&self, access: impl FnOnce(IoContext<'_>) -> R) -> R {
        self.state
            .runtime
            .with_runtime(|wasm| access(IoContext::new(wasm.context(), &self.state.identity)))
    }

    /// Acknowledges the first ready interrupt in connection order.
    pub fn poll_interrupt(&self, cx: &mut Context<'_>) -> Poll<u8> {
        self.with_context(|context| self.state.poll_interrupt(context, cx))
    }

    /// Reads one through eight bytes, splitting only at device boundaries.
    pub fn read_mmio(&self, address: u32, bytes: u8) -> u64 {
        self.with_context(|context| self.state.read_mmio(context, address, bytes))
    }

    /// Writes the low `bytes` bytes, splitting only at device boundaries.
    pub fn write_mmio(&self, address: u32, bytes: u8, value: u64) {
        self.with_context(|context| self.state.write_mmio(context, address, bytes, value))
    }

    /// Reads 1, 2 or 4 bytes from the device at `port` in one call. Unused result
    /// bits are zero; the device owns the meaning of word and dword transfers.
    /// Panics unless `bytes` is 1, 2 or 4.
    pub fn read_port(&self, port: u16, bytes: u8) -> u32 {
        self.with_context(|context| self.state.read_port(context, port, bytes))
    }

    /// Writes the low 1, 2 or 4 bytes to the device at `port` in one call.
    /// Panics unless `bytes` is 1, 2 or 4.
    pub fn write_port(&self, port: u16, bytes: u8, value: u32) {
        self.with_context(|context| self.state.write_port(context, port, bytes, value))
    }

    /// Claims this bus for one CPU, including if runtime initialization fails.
    pub(crate) fn attach_cpu(&self) -> crate::Result<WasmRuntimeHandle> {
        assert!(
            !self.state.cpu_attached.replace(true),
            "an I/O bus supports exactly one CPU"
        );
        self.state.runtime.initialize()?;
        Ok(self.state.runtime.clone())
    }

    pub(crate) fn port_map(&self) -> &ports::Ports {
        &self.state.ports
    }

    pub(crate) fn downgrade(&self) -> Weak<State> {
        self.state.identity.clone()
    }
}

impl State {
    pub fn is_empty(&self) -> bool {
        self.connections.borrow().is_empty()
    }

    pub fn poll_interrupt(&self, mut context: IoContext<'_>, cx: &mut Context<'_>) -> Poll<u8> {
        for connection in self.connections.borrow().iter() {
            if let Poll::Ready(vector) = connection
                .device
                .borrow_mut()
                .poll_interrupt(context.reborrow(), cx)
            {
                return Poll::Ready(vector);
            }
        }
        Poll::Pending
    }

    pub fn read_mmio(&self, mut context: IoContext<'_>, address: u32, bytes: u8) -> u64 {
        check_mmio_range(address, bytes);
        let connections = self.connections.borrow();
        let mut result = [0xff; 8];
        let mut done = 0;
        while done < bytes {
            let current = address + u32::from(done);
            let (device, count) = Self::mmio_span(&connections, current, bytes - done);
            if let Some(device) = device {
                let value = device
                    .borrow_mut()
                    .read_mmio(context.reborrow(), current, count)
                    .to_le_bytes();
                result[usize::from(done)..usize::from(done + count)]
                    .copy_from_slice(&value[..usize::from(count)]);
            }
            done += count;
        }
        // Unused carrier bytes do not form part of the transfer.
        result[usize::from(bytes)..].fill(0);
        u64::from_le_bytes(result)
    }

    pub fn write_mmio(&self, mut context: IoContext<'_>, address: u32, bytes: u8, value: u64) {
        check_mmio_range(address, bytes);
        let connections = self.connections.borrow();
        let value = value.to_le_bytes();
        let mut done = 0;
        while done < bytes {
            let current = address + u32::from(done);
            let (device, count) = Self::mmio_span(&connections, current, bytes - done);
            if let Some(device) = device {
                let mut part = [0; 8];
                part[..usize::from(count)]
                    .copy_from_slice(&value[usize::from(done)..usize::from(done + count)]);
                device.borrow_mut().write_mmio(
                    context.reborrow(),
                    current,
                    count,
                    u64::from_le_bytes(part),
                );
            }
            done += count;
        }
    }

    pub fn read_port(&self, context: IoContext<'_>, port: u16, bytes: u8) -> u32 {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        self.ports.read(context, port, bytes) & (u32::MAX >> ((4 - bytes) * 8))
    }

    pub fn write_port(&self, context: IoContext<'_>, port: u16, bytes: u8, value: u32) {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        self.ports.write(
            context,
            port,
            bytes,
            value & (u32::MAX >> ((4 - bytes) * 8)),
        );
    }

    fn mmio_span(
        connections: &[ConnectedDevice],
        address: u32,
        bytes: u8,
    ) -> (Option<&RefCell<dyn Device>>, u8) {
        let mut count = u64::from(bytes);
        for connection in connections {
            for range in &connection.mmio {
                if range.contains(&address) {
                    count = count.min(u64::from(*range.end()) - u64::from(address) + 1);
                    return (Some(connection.device.as_ref()), count as u8);
                }
                if *range.start() > address {
                    count = count.min(u64::from(*range.start()) - u64::from(address));
                }
            }
        }
        (None, count as u8)
    }
}

fn validate_ranges<'a, T: Ord + Copy + 'a>(ranges: impl Iterator<Item = &'a RangeInclusive<T>>) {
    let mut ranges: Vec<_> = ranges.collect();
    assert!(
        ranges.iter().all(|range| !range.is_empty()),
        "empty device range"
    );
    ranges.sort_unstable_by_key(|range| *range.start());
    assert!(
        ranges
            .windows(2)
            .all(|pair| pair[0].end() < pair[1].start()),
        "overlapping device ranges"
    );
}

fn check_mmio_range(address: u32, bytes: u8) {
    assert!((1..=8).contains(&bytes), "MMIO transfer width");
    assert!(
        u64::from(address) + u64::from(bytes) <= 1 << 32,
        "MMIO address range"
    );
}
