//! Owns one CPU's port tables and applies edits at configuration boundaries.

mod bindings;

use super::{Backend, Context};
use crate::{
    io::ports::{width_mask, Read, Write},
    PortRead, PortWrite,
};
use bindings::Bindings;
use relocade_wasm_runtime::{FunctionRef, Result, Table, WasmBackend, WasmContext};
use relocade_x86::abi;
use std::ops::RangeInclusive;

type ReadFunction = FunctionRef<Backend, abi::PortReadSignature>;
type WriteFunction = FunctionRef<Backend, abi::PortWriteSignature>;

pub(super) struct PortTables {
    pub(super) reads: Table<Backend, abi::PortReadSignature>,
    pub(super) writes: Table<Backend, abi::PortWriteSignature>,
    read_default: ReadFunction,
    write_default: WriteFunction,
    read_bindings: Bindings<Read, ReadFunction>,
    write_bindings: Bindings<Write, WriteFunction>,
}

impl PortTables {
    pub(super) fn new(backend: &mut Backend) -> Result<Self> {
        let read_default =
            backend.host_function::<abi::PortReadSignature>(Box::new(|context, port, bytes| {
                context.state().io.read_port(port as u16, bytes as u8) as i32
            }));
        let write_default = backend.host_function::<abi::PortWriteSignature>(Box::new(
            |context, port, bytes, value| {
                context
                    .state()
                    .io
                    .write_port(port as u16, bytes as u8, value as u32);
            },
        ));
        Ok(Self {
            reads: backend.create_table(abi::PORT_READS, Some(&read_default))?,
            writes: backend.create_table(abi::PORT_WRITES, Some(&write_default))?,
            read_default,
            write_default,
            read_bindings: Bindings::default(),
            write_bindings: Bindings::default(),
        })
    }
}

/// Scoped access to one CPU's live port tables, obtained from
/// [`crate::Cpu::port_mappings`]. Edits take effect immediately in already-linked
/// code and the interpreter. Unbound entries use the generic [`crate::IoBus`].
/// The editor cannot be retained in device state or shared with another CPU.
/// Generic device and host-bus accesses retain their original routing.
///
/// ```
/// use relocade_machine::{Cpu, PortRead};
/// fn configure(cpu: &mut Cpu) {
///     cpu.port_mappings().set_read(0x80..=0x80, Some(PortRead::new(|_, _| 0x5a)));
///     cpu.port_mappings().set_read(0x80..=0x80, None); // Restore generic routing.
/// }
/// ```
pub struct PortMappings<'a> {
    context: Context<'a>,
}

impl<'a> PortMappings<'a> {
    pub(in crate::runtime) fn new(context: Context<'a>) -> Self {
        Self { context }
    }

    /// Overrides reads by their starting port. `None` restores generic routing.
    /// A wide transfer remains one operation; the handler owns byte ordering.
    /// An empty range is a programming error.
    pub fn set_read(&mut self, ports: RangeInclusive<u16>, handler: Option<PortRead>) {
        assert!(!ports.is_empty(), "empty port range");
        // Only configuration needs mutable engine access. Taking the owner out
        // permits callback creation and table edits without borrowing backend state.
        let mut tables = self.context.state_mut().ports.take().unwrap();
        for port in ports {
            let function = match &handler {
                Some(handler) => tables.read_bindings.set(port, handler.0.clone(), |slot| {
                    read_function(&mut self.context, slot)
                }),
                None => {
                    tables.read_bindings.remove(port);
                    tables.read_default.clone()
                }
            };
            tables
                .reads
                .set(&mut self.context, u32::from(port), &function)
                .expect("port read table slot");
        }
        self.context.state_mut().ports = Some(tables);
    }

    /// Overrides writes by their starting port. `None` restores generic routing.
    /// An empty range is a programming error.
    pub fn set_write(&mut self, ports: RangeInclusive<u16>, handler: Option<PortWrite>) {
        assert!(!ports.is_empty(), "empty port range");
        let mut tables = self.context.state_mut().ports.take().unwrap();
        for port in ports {
            let function = match &handler {
                Some(handler) => tables.write_bindings.set(port, handler.0.clone(), |slot| {
                    write_function(&mut self.context, slot)
                }),
                None => {
                    tables.write_bindings.remove(port);
                    tables.write_default.clone()
                }
            };
            tables
                .writes
                .set(&mut self.context, u32::from(port), &function)
                .expect("port write table slot");
        }
        self.context.state_mut().ports = Some(tables);
    }
}

fn read_function(context: &mut Context<'_>, slot: usize) -> ReadFunction {
    ReadFunction::host(
        context,
        Box::new(move |context, port, bytes| {
            let mask = width_mask(bytes as u8);
            let handler = context
                .state()
                .ports
                .as_ref()
                .unwrap()
                .read_bindings
                .handler(slot);
            (handler(port as u16, bytes as u8) & mask) as i32
        }),
    )
}

fn write_function(context: &mut Context<'_>, slot: usize) -> WriteFunction {
    WriteFunction::host(
        context,
        Box::new(move |context, port, bytes, value| {
            let mask = width_mask(bytes as u8);
            let handler = context
                .state()
                .ports
                .as_ref()
                .unwrap()
                .write_bindings
                .handler(slot);
            handler(port as u16, bytes as u8, value as u32 & mask);
        }),
    )
}
