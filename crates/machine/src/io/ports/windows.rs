//! Fixed address windows own transfer boundaries, holes and decoding gates.
//! Reconfiguration publishes cached operations; windows never participate in access.

use super::{IndexedPorts, Port, PortBinding, PortPolicy, PortRead, PortValue, PortWrite};
use crate::IoContext;
use std::ops::RangeInclusive;

/// A fixed address window with an explicit transfer policy. The adapter supplies
/// individual endpoints to its components and retains the window to configure
/// holes and decoding. Dropping it does not release claims or change handlers.
///
/// ```
/// use relocade_machine::{Device, IoBus, PortPolicy};
/// use std::{cell::RefCell, rc::Rc};
/// struct Registers;
/// impl Device for Registers {}
/// let io = IoBus::new(Rc::new(RefCell::new(Registers)), |io| {
///     let mut window = io.ports(0x80..=0x82, PortPolicy::BYTE_SEQUENCE);
///     let register = window.port(0x81).define(|_| 7, |_, _| {});
///     window.configure(io.context(), true, |context| {
///         register.configure(context, &register.read, &register.write);
///     });
/// });
/// // A word beginning at the hole still reaches the implemented register.
/// assert_eq!(io.read_port(0x80, 2), 0x07ff);
/// ```
pub struct PortWindow<T> {
    binding: PortBinding,
    policy: PortPolicy<T>,
    holes: Vec<RangeInclusive<u16>>,
    defaults: Handlers,
    disabled: Handlers,
}

struct Handlers {
    read: PortRead,
    write: PortWrite,
}

impl Handlers {
    fn open_bus() -> Self {
        Self {
            read: PortRead::new(|_, _, _| u32::MAX),
            write: PortWrite::new(|_, _, _, _| {}),
        }
    }

    fn configure(&self, binding: &PortBinding, mut context: IoContext<'_>) {
        binding.set_read(context.reborrow(), &self.read);
        binding.set_write(context, &self.write);
    }
}

impl<T: PortValue> PortWindow<T> {
    pub(in crate::io) fn new(binding: PortBinding, policy: PortPolicy<T>) -> Self {
        let defaults = Handlers {
            read: policy.read(binding.ports.clone(), |_| T::from_u32(u32::MAX)),
            write: policy.write(binding.ports.clone(), |_, _| {}),
        };
        Self {
            holes: vec![binding.ports.clone()],
            binding,
            policy,
            defaults,
            disabled: Handlers::open_bus(),
        }
    }

    /// Gives a component one endpoint, retaining this window's transfer boundary.
    /// Undeclared addresses remain holes. Every endpoint can be declared only once.
    /// Its initial definitions return FF and ignore writes while still processing
    /// the tail of a sequential transfer.
    pub fn port(&mut self, address: u16) -> Port<T> {
        assert!(self.binding.ports.contains(&address), "port outside window");
        let position = self
            .holes
            .iter()
            .position(|range| range.contains(&address))
            .expect("port already declared");
        let range = self.holes.remove(position);
        if *range.start() < address {
            self.holes.push(*range.start()..=address - 1);
        }
        if address < *range.end() {
            self.holes.push(address + 1..=*range.end());
        }
        Port {
            binding: PortBinding::new(&self.binding.bus, address..=address),
            policy: self.policy,
            end: *self.binding.ports.end(),
            read: self.defaults.read.clone(),
            write: self.defaults.write.clone(),
        }
    }

    /// Gives a component an index/data pair without narrowing its transfer boundary.
    pub fn indexed(&mut self, index: u16, data: u16) -> IndexedPorts<T> {
        assert_ne!(index, data, "index and data require distinct ports");
        IndexedPorts {
            index: self.port(index),
            data: self.port(data),
        }
    }

    /// Configures holes and asks components to publish their final operations.
    /// The callback must configure every declared endpoint from current state.
    /// It runs synchronously after releasing map borrows, at connection or a
    /// decoding/mode boundary. Existing endpoint handlers are not reset first.
    ///
    /// Disabled windows ignore complete transfers, including those starting in
    /// holes. Their callback is skipped. Re-enabling configures holes and all
    /// endpoints again; ordinary accesses never consult a window or enabled flag.
    pub fn configure(
        &self,
        mut context: IoContext<'_>,
        enabled: bool,
        configure: impl FnOnce(IoContext<'_>),
    ) {
        if enabled {
            for hole in &self.holes {
                let binding = PortBinding::new(&self.binding.bus, hole.clone());
                self.defaults.configure(&binding, context.reborrow());
            }
            configure(context);
        } else {
            self.disabled.configure(&self.binding, context);
        }
    }
}
