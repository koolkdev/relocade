//! Adapter-wide decoding and connection of the individual register components.
//! Windows stay here because transfers and decoding gates cross components.

mod status;
#[cfg(test)]
mod tests;

use crate::{registers::*, Vga, MEMORY_RANGE};
use relocade_machine::{
    BytePort, DeviceCallbacks, DeviceConnection, IoContext, PortPolicy, PortRead, PortWindow,
    PortWrite,
};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

pub(super) struct Ports {
    common: PortWindow<u8>,
    common_registers: [BytePort; 3],
    dispi: PortWindow<u16>,
    mono: Alias,
    color: Alias,
}

/// CRTC and Input Status 1 decode together at either monochrome or color ports.
struct Alias {
    crtc: PortWindow<u8>,
    disabled_read: PortRead,
    disabled_write: PortWrite,
    status: BytePort,
    packed_read: PortRead,
}

impl Vga {
    /// Claims the adapter's windows and gives fixed endpoints to its components.
    /// Initial wiring and subsequent state changes use the same configuration.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        let mut vga = device.borrow_mut();
        assert!(vga.ports.is_none(), "VGA is already connected");
        io.map_mmio([MEMORY_RANGE]);
        let callbacks = DeviceCallbacks::new(device);
        let mut common = io.ports(
            ATTRIBUTE_ADDRESS_PORT..=GRAPHICS_DATA_PORT,
            PortPolicy::BYTE_SEQUENCE,
        );
        let mut dispi = io.ports(dispi::PORT_RANGE, PortPolicy::WORD);
        let mut mono = Alias::new(
            io,
            &vga,
            device,
            &callbacks,
            MONO_CRTC_INDEX_PORT..=MONO_CRTC_DATA_PORT,
            MONO_INPUT_STATUS_1_PORT,
        );
        let mut color = Alias::new(
            io,
            &vga,
            device,
            &callbacks,
            CRTC_PORT_RANGE,
            COLOR_INPUT_STATUS_1_PORT,
        );
        vga.registers.attribute.connect(
            common.port(ATTRIBUTE_ADDRESS_PORT),
            common.port(ATTRIBUTE_DATA_READ_PORT),
            &callbacks,
        );
        vga.registers.sequencer.connect(
            common.indexed(SEQUENCER_INDEX_PORT, SEQUENCER_DATA_PORT),
            &callbacks,
        );
        vga.registers.graphics.connect(
            common.indexed(GRAPHICS_INDEX_PORT, GRAPHICS_DATA_PORT),
            &callbacks,
        );
        vga.registers.dac.connect(
            common.port(DAC_MASK_PORT),
            common.port(DAC_READ_ADDRESS_PORT),
            common.port(DAC_WRITE_ADDRESS_PORT),
            common.port(DAC_DATA_PORT),
            &callbacks,
        );
        vga.registers.crtc.connect(
            mono.crtc.indexed(MONO_CRTC_INDEX_PORT, MONO_CRTC_DATA_PORT),
            color.crtc.indexed(CRTC_INDEX_PORT, CRTC_DATA_PORT),
            &callbacks,
        );
        vga.dispi.connect(
            dispi.indexed(dispi::INDEX_PORT, dispi::DATA_PORT),
            &callbacks,
        );
        setup::Setup::connect(io, &callbacks);
        let common_registers = [
            common.port(MISC_OUTPUT_WRITE_PORT).define(
                // Attached color display; no retrace IRQ wired.
                |_| 0x10,
                callbacks.write(write_misc_output),
            ),
            common.port(MISC_OUTPUT_READ_PORT).define(
                callbacks.read(|_, vga| vga.registers.miscellaneous),
                |_, _| {},
            ),
            common.port(FEATURE_CONTROL_READ_PORT).define(
                callbacks.read(|_, vga| vga.registers.feature_control),
                |_, _| {},
            ),
        ];
        vga.ports = Some(Ports {
            common,
            common_registers,
            dispi,
            mono,
            color,
        });
        vga.configure_ports(io.context());
    }

    pub(super) fn configure_ports(&self, mut context: IoContext<'_>) {
        let Some(ports) = &self.ports else { return };
        let enabled = self.setup.state.enabled();
        let color = self.registers.status_port() == COLOR_INPUT_STATUS_1_PORT;
        let packed = self.dispi.state.display_layout().is_some();
        ports
            .common
            .configure(context.reborrow(), enabled, |mut context| {
                for port in &ports.common_registers {
                    port.configure(context.reborrow(), &port.read, &port.write);
                }
                self.registers
                    .attribute
                    .configure(context.reborrow(), packed);
                self.registers
                    .sequencer
                    .configure(context.reborrow(), packed);
                self.registers
                    .graphics
                    .configure(context.reborrow(), packed);
                self.registers.dac.configure(context);
            });
        ports
            .dispi
            .configure(context.reborrow(), enabled, |context| {
                self.dispi.configure(context)
            });
        for (alias, is_color) in [(&ports.mono, false), (&ports.color, true)] {
            let active = enabled && color == is_color;
            alias.crtc.configure(context.reborrow(), active, |context| {
                self.registers
                    .crtc
                    .configure_alias(context, is_color, packed);
            });
            let (read, write) = if active {
                let read = if packed {
                    &alias.packed_read
                } else {
                    &alias.status.read
                };
                (read, &alias.status.write)
            } else {
                (&alias.disabled_read, &alias.disabled_write)
            };
            alias.status.configure(context.reborrow(), read, write);
        }
    }
}

impl Alias {
    fn new(
        io: &mut DeviceConnection<'_>,
        vga: &Vga,
        device: &Rc<RefCell<Vga>>,
        callbacks: &DeviceCallbacks<Vga>,
        crtc: RangeInclusive<u16>,
        status_address: u16,
    ) -> Self {
        let crtc = io.ports(crtc, PortPolicy::BYTE_SEQUENCE);
        let status = io.port(
            status_address,
            PortPolicy::BYTE_SEQUENCE,
            status::read(vga, device, Vga::refresh_vga_raster),
            callbacks.write(|_, vga, value| vga.registers.feature_control = value),
        );
        let packed_read = status.read_handler(status::read(vga, device, Vga::refresh_svga_raster));
        let disabled_read = status.read_handler(|_| 0xff);
        let disabled_write = status.write_handler(|_, _| {});
        Self {
            crtc,
            status,
            packed_read,
            disabled_read,
            disabled_write,
        }
    }
}

fn write_misc_output(context: IoContext<'_>, vga: &mut Vga, value: u8) {
    vga.advance_raster(vga.clock.now());
    let previous_status_port = vga.registers.status_port();
    vga.registers.miscellaneous = value;
    vga.configure_display(context, vga.dispi.state.display_layout());
    if vga.registers.status_port() != previous_status_port {
        vga.programming_changed.notify(usize::MAX);
    }
}
