//! Component-owned endpoints and reusable operations, standalone or within windows.

use super::{PortBinding, PortPolicy, PortRead, PortValue, PortWrite};
use crate::IoContext;

/// A byte register, registered directly or supplied by a fixed window.
pub type BytePort = Port<u8>;
/// A word register, registered directly or supplied by a fixed window.
pub type WordPort = Port<u16>;

/// A fixed endpoint and its declared reusable operations. The `read` and `write`
/// fields are definitions, not a mirror of the currently installed handlers.
/// Configuration publishes an explicit final pair; variants can be prepared once
/// and retained by the component. An endpoint never retains an execution context.
#[derive(Clone)]
pub struct Port<T> {
    pub(super) binding: PortBinding,
    pub(super) policy: PortPolicy<T>,
    pub(super) end: u16,
    /// The declared read operation, reusable in later configurations.
    pub read: PortRead,
    /// The declared write operation, reusable in later configurations.
    pub write: PortWrite,
}

impl<T: PortValue> Port<T> {
    pub(in crate::io) fn new(
        binding: PortBinding,
        policy: PortPolicy<T>,
        read: impl Fn(IoContext<'_>) -> T + 'static,
        write: impl Fn(IoContext<'_>, T) + 'static,
    ) -> Self {
        Self {
            read: policy.read(binding.ports.clone(), read),
            write: policy.write(binding.ports.clone(), write),
            end: *binding.ports.end(),
            binding,
            policy,
        }
    }

    /// Defines both operations on an endpoint supplied by a window. Use an FF
    /// read or no-op write for a direction the device does not implement. These
    /// callbacks still follow the endpoint's transfer policy. No handler is
    /// installed until the component configures its final pair.
    pub fn define(
        mut self,
        read: impl Fn(IoContext<'_>) -> T + 'static,
        write: impl Fn(IoContext<'_>, T) + 'static,
    ) -> Self {
        self.read = self.read_handler(read);
        self.write = self.write_handler(write);
        self
    }

    /// Publishes the final read/write pair directly, without an intermediate
    /// reset. Use the declared operations or variants prepared by this endpoint.
    pub fn configure(&self, mut context: IoContext<'_>, read: &PortRead, write: &PortWrite) {
        self.set_read(context.reborrow(), read);
        self.set_write(context, write);
    }

    /// Changes only the read operation at a device state boundary.
    pub fn set_read(&self, context: IoContext<'_>, read: &PortRead) {
        self.binding.set_read(context, read);
    }

    /// Changes only the write operation at a device state boundary.
    pub fn set_write(&self, context: IoContext<'_>, write: &PortWrite) {
        self.binding.set_write(context, write);
    }

    /// Prepares a reusable variant, preserving the concrete callback and this
    /// endpoint's transfer boundary. Preparation does not change the port map.
    pub fn read_handler(&self, read: impl Fn(IoContext<'_>) -> T + 'static) -> PortRead {
        self.policy
            .read(*self.binding.ports.start()..=self.end, read)
    }

    pub fn write_handler(&self, write: impl Fn(IoContext<'_>, T) + 'static) -> PortWrite {
        self.policy
            .write(*self.binding.ports.start()..=self.end, write)
    }
}
