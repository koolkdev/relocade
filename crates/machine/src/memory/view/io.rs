use crate::{IoBus, IoContext};
use std::cell::RefCell;

pub(super) enum Routing<'a> {
    Bus(&'a IoBus),
    Context {
        bus: IoBus,
        context: RefCell<IoContext<'a>>,
    },
}

impl Routing<'_> {
    pub(super) fn bus(&self) -> &IoBus {
        match self {
            Self::Bus(bus) => bus,
            Self::Context { bus, .. } => bus,
        }
    }

    pub(super) fn read_mmio(&self, address: u32, bytes: u8) -> u64 {
        match self {
            Self::Bus(bus) => bus.read_mmio(address, bytes),
            Self::Context { context, .. } => context.borrow_mut().read_mmio(address, bytes),
        }
    }

    pub(super) fn write_mmio(&self, address: u32, bytes: u8, value: u64) {
        match self {
            Self::Bus(bus) => bus.write_mmio(address, bytes, value),
            Self::Context { context, .. } => {
                context.borrow_mut().write_mmio(address, bytes, value);
            }
        }
    }
}
