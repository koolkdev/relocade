//! Fixed bindings select individual register operations. Address-decoding changes
//! replace cached handlers here, before the next guest access.

mod access;
mod operations;
mod status;

use crate::{registers::*, Vga, MEMORY_RANGE};
use operations::*;
use relocade_machine::{DeviceConnection, IoContext, PortBinding, PortRead, PortWrite};
use std::{cell::RefCell, rc::Rc};

pub(super) struct Ports {
    common: Vec<RegisterPort>,
    mono: [RegisterPort; 2],
    color: [RegisterPort; 2],
    mono_status: PortBinding,
    color_status: PortBinding,
    vga_status: PortRead,
    svga_status: PortRead,
    feature_control: PortWrite,
    open_bus: PortRead,
    ignore_write: PortWrite,
}

struct RegisterPort {
    binding: PortBinding,
    read: PortRead,
    write: PortWrite,
}

impl Vga {
    /// Maps this device's aperture and ports during its one connection scope.
    /// Both manually assembled devices and [`Self::initialize`] use this wiring.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        let mut vga = device.borrow_mut();
        assert!(vga.ports.is_none(), "VGA is already connected");
        io.map_mmio([MEMORY_RANGE]);

        // Setup registers remain accessible when normal VGA decoding is disabled.
        for port in [
            RegisterPort::byte(
                io,
                device,
                OPTION_SELECT_PORT,
                OPTION_SELECT_PORT,
                read_option,
                write_option,
            ),
            RegisterPort::byte(
                io,
                device,
                ADAPTER_CONTROL_PORT,
                ADAPTER_CONTROL_PORT,
                read_open_bus,
                write_adapter_control,
            ),
        ] {
            port.binding.set_read(io.context(), &port.read);
            port.binding.set_write(io.context(), &port.write);
        }

        let common = vec![
            RegisterPort::byte(
                io,
                device,
                ATTRIBUTE_ADDRESS_PORT,
                GRAPHICS_DATA_PORT,
                read_attribute_address,
                write_attribute,
            ),
            RegisterPort::byte(
                io,
                device,
                ATTRIBUTE_DATA_READ_PORT,
                GRAPHICS_DATA_PORT,
                read_attribute_data,
                ignore_write,
            ),
            RegisterPort::byte(
                io,
                device,
                MISC_OUTPUT_WRITE_PORT,
                GRAPHICS_DATA_PORT,
                read_input_status_0,
                write_misc_output,
            ),
            RegisterPort::byte(
                io,
                device,
                0x3c3,
                GRAPHICS_DATA_PORT,
                read_open_bus,
                ignore_write,
            ),
            RegisterPort::byte(
                io,
                device,
                SEQUENCER_INDEX_PORT,
                GRAPHICS_DATA_PORT,
                read_sequencer_index,
                write_sequencer_index,
            ),
            RegisterPort::byte(
                io,
                device,
                SEQUENCER_DATA_PORT,
                GRAPHICS_DATA_PORT,
                read_sequencer_data,
                write_sequencer_data,
            ),
            RegisterPort::byte(
                io,
                device,
                DAC_MASK_PORT,
                GRAPHICS_DATA_PORT,
                read_dac_mask,
                write_dac_mask,
            ),
            RegisterPort::byte(
                io,
                device,
                DAC_READ_ADDRESS_PORT,
                GRAPHICS_DATA_PORT,
                read_dac_state,
                write_dac_read_address,
            ),
            RegisterPort::byte(
                io,
                device,
                DAC_WRITE_ADDRESS_PORT,
                GRAPHICS_DATA_PORT,
                read_dac_address,
                write_dac_write_address,
            ),
            RegisterPort::byte(
                io,
                device,
                DAC_DATA_PORT,
                GRAPHICS_DATA_PORT,
                read_dac_data,
                write_dac_data,
            ),
            RegisterPort::byte(
                io,
                device,
                FEATURE_CONTROL_READ_PORT,
                GRAPHICS_DATA_PORT,
                read_feature_control,
                ignore_write,
            ),
            RegisterPort::byte(
                io,
                device,
                0x3cb,
                GRAPHICS_DATA_PORT,
                read_open_bus,
                ignore_write,
            ),
            RegisterPort::byte(
                io,
                device,
                MISC_OUTPUT_READ_PORT,
                GRAPHICS_DATA_PORT,
                read_misc_output,
                ignore_write,
            ),
            RegisterPort::byte(
                io,
                device,
                0x3cd,
                GRAPHICS_DATA_PORT,
                read_open_bus,
                ignore_write,
            ),
            RegisterPort::byte(
                io,
                device,
                GRAPHICS_INDEX_PORT,
                GRAPHICS_DATA_PORT,
                read_graphics_index,
                write_graphics_index,
            ),
            RegisterPort::byte(
                io,
                device,
                GRAPHICS_DATA_PORT,
                GRAPHICS_DATA_PORT,
                read_graphics_data,
                write_graphics_data,
            ),
            RegisterPort {
                binding: io.port(dispi::INDEX_PORT),
                read: access::word_read(device, read_dispi_index),
                write: access::word_write(device, write_dispi_index),
            },
            RegisterPort {
                binding: io.port(dispi::DATA_PORT),
                read: access::word_read(device, read_dispi_data),
                write: access::word_write(device, write_dispi_data),
            },
        ];
        let mono = crtc_ports(io, device, MONO_CRTC_INDEX_PORT, MONO_CRTC_DATA_PORT);
        let color = crtc_ports(io, device, CRTC_INDEX_PORT, CRTC_DATA_PORT);
        vga.ports = Some(Ports {
            common,
            mono,
            color,
            mono_status: io.port(MONO_INPUT_STATUS_1_PORT),
            color_status: io.port(COLOR_INPUT_STATUS_1_PORT),
            vga_status: status::read(&vga, device, Vga::refresh_vga_raster),
            svga_status: status::read(&vga, device, Vga::refresh_svga_raster),
            feature_control: status::write(device),
            open_bus: PortRead::new(|_, _, _| u32::MAX),
            ignore_write: PortWrite::new(|_, _, _, _| {}),
        });
        vga.configure_ports(io.context());
    }

    pub(super) fn configure_ports(&self, mut context: IoContext<'_>) {
        let Some(ports) = &self.ports else {
            return;
        };
        let enabled = self.setup.enabled();
        let color = self.registers.status_port() == COLOR_INPUT_STATUS_1_PORT;
        for (group, active) in [
            (ports.common.as_slice(), enabled),
            (ports.mono.as_slice(), enabled && !color),
            (ports.color.as_slice(), enabled && color),
        ] {
            for port in group {
                port.binding.set_read(
                    context.reborrow(),
                    if active { &port.read } else { &ports.open_bus },
                );
                port.binding.set_write(
                    context.reborrow(),
                    if active {
                        &port.write
                    } else {
                        &ports.ignore_write
                    },
                );
            }
        }
        let status = if self.dispi.display_layout().is_some() {
            &ports.svga_status
        } else {
            &ports.vga_status
        };
        for (binding, active) in [
            (&ports.mono_status, enabled && !color),
            (&ports.color_status, enabled && color),
        ] {
            binding.set_read(
                context.reborrow(),
                if active { status } else { &ports.open_bus },
            );
            binding.set_write(
                context.reborrow(),
                if active {
                    &ports.feature_control
                } else {
                    &ports.ignore_write
                },
            );
        }
    }
}

impl RegisterPort {
    fn byte(
        io: &mut DeviceConnection<'_>,
        device: &Rc<RefCell<Vga>>,
        address: u16,
        last_port: u16,
        read: impl Fn(&mut Vga) -> u8 + 'static,
        write: impl Fn(&mut Vga, IoContext<'_>, u8) + 'static,
    ) -> Self {
        Self {
            binding: io.port(address),
            read: access::byte_read(device, last_port, read),
            write: access::byte_write(device, last_port, write),
        }
    }
}

fn crtc_ports(
    io: &mut DeviceConnection<'_>,
    device: &Rc<RefCell<Vga>>,
    index: u16,
    data: u16,
) -> [RegisterPort; 2] {
    [
        RegisterPort::byte(io, device, index, data, read_crtc_index, write_crtc_index),
        RegisterPort::byte(io, device, data, data, read_crtc_data, write_crtc_data),
    ]
}
