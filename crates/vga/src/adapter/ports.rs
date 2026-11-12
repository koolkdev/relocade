//! Connection-time wiring and changes to physical address decoding.

mod status;

use crate::{dispi, registers::*, DispiAdapter, MEMORY_RANGE};
use relocade_machine::{
    DeviceCallbacks, DeviceConnection, IoContext, PortBinding, PortRange, PortRead, PortWrite,
};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

const COMMON: RangeInclusive<u16> = ATTRIBUTE_ADDRESS_PORT..=GRAPHICS_DATA_PORT;

/// Effective address routing; either active state also supports packed scanout.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PortDecoding {
    Disabled,
    Monochrome,
    Color,
}

#[derive(Clone, Copy)]
enum PortGroup {
    Common,
    Dispi,
    Monochrome,
    Color,
}

impl PortDecoding {
    fn enables(self, group: PortGroup) -> bool {
        match self {
            Self::Disabled => false,
            Self::Monochrome => matches!(
                group,
                PortGroup::Common | PortGroup::Dispi | PortGroup::Monochrome
            ),
            Self::Color => matches!(
                group,
                PortGroup::Common | PortGroup::Dispi | PortGroup::Color
            ),
        }
    }
}

/// Retained only for decoding changes. Ordinary accesses invoke the installed
/// operations directly, without consulting this companion or its mappings.
pub(super) struct Io {
    common: Vec<Mapping>,
    dispi: [Mapping; 2],
    mono: Vec<Mapping>,
    color: Vec<Mapping>,
    disabled_read: PortRead,
    disabled_write: PortWrite,
}

struct Mapping {
    binding: PortBinding,
    active_read: PortRead,
    active_write: PortWrite,
}

impl Mapping {
    fn bind(
        io: &mut DeviceConnection<'_>,
        ports: impl Into<PortRange>,
        read: PortRead,
        write: PortWrite,
    ) -> Self {
        Self {
            binding: io.bind(ports, &read, &write),
            active_read: read,
            active_write: write,
        }
    }

    fn byte(
        io: &mut DeviceConnection<'_>,
        window: RangeInclusive<u16>,
        ports: impl Into<PortRange>,
        read: impl Fn(IoContext<'_>) -> u8 + 'static,
        write: impl Fn(IoContext<'_>, u8) + 'static,
    ) -> Self {
        Self::bind(
            io,
            ports,
            PortRead::byte_sequence(window.clone(), read),
            PortWrite::byte_sequence(window, write),
        )
    }
}

impl DispiAdapter {
    /// Connects VGA's MMIO aperture and individual port operations. VGA sequences
    /// retain their physical windows; setup/status consume the low byte, and
    /// DISPI accepts exact word accesses.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "VGA is already connected");
        io.map_mmio([MEMORY_RANGE]);
        let callbacks = DeviceCallbacks::new(device);
        let common = bind_common_vga_ports(io, &callbacks);
        let dispi = bind_dispi_ports(io, &callbacks);
        // Setup remains accessible while the adapter's other addresses are off.
        io.bind(
            OPTION_SELECT_PORT,
            &PortRead::low_byte(callbacks.read(|_, card| card.vga.setup.read_option())),
            &PortWrite::low_byte(
                callbacks.write(|context, card, value| card.write_option_select(context, value)),
            ),
        );
        io.bind(
            ADAPTER_CONTROL_PORT,
            &PortRead::new(|_, _, _| u32::MAX),
            &PortWrite::low_byte(
                callbacks.write(|context, card, value| card.write_adapter_control(context, value)),
            ),
        );
        let mono = crtc(
            io,
            device,
            &callbacks,
            MONO_CRTC_INDEX_PORT,
            MONO_INPUT_STATUS_1_PORT,
        );
        let color = crtc(
            io,
            device,
            &callbacks,
            CRTC_INDEX_PORT,
            COLOR_INPUT_STATUS_1_PORT,
        );
        let ports = Io {
            common,
            dispi,
            mono,
            color,
            // These consume the entire transfer, unlike enabled-window holes.
            disabled_read: PortRead::new(|_, _, _| u32::MAX),
            disabled_write: PortWrite::new(|_, _, _, _| {}),
        };
        let mut card = device.borrow_mut();
        ports.update_decoding(io.context(), None, card.decoding());
        card.io = Some(ports);
    }

    pub(super) fn decoding(&self) -> PortDecoding {
        if !self.vga.setup.enabled() {
            PortDecoding::Disabled
        } else if self.vga.registers.miscellaneous & 1 != 0 {
            PortDecoding::Color
        } else {
            PortDecoding::Monochrome
        }
    }
}

fn bind_common_vga_ports(
    io: &mut DeviceConnection<'_>,
    callbacks: &DeviceCallbacks<DispiAdapter>,
) -> Vec<Mapping> {
    // Enabled holes must continue wide transfers to later implemented ports.
    vec![
        Mapping::byte(io, COMMON, COMMON, |_| 0xff, |_, _| {}),
        Mapping::byte(
            io,
            COMMON,
            ATTRIBUTE_ADDRESS_PORT,
            callbacks.read(|_, card| card.vga.registers.attribute.address()),
            callbacks.write(|context, card, value| card.write_attribute(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            ATTRIBUTE_DATA_READ_PORT,
            callbacks.read(|_, card| card.vga.registers.attribute.read()),
            |_, _| {},
        ),
        Mapping::byte(
            io,
            COMMON,
            MISC_OUTPUT_WRITE_PORT,
            |_| 0x10,
            callbacks.write(|context, card, value| card.write_misc_output(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            SEQUENCER_INDEX_PORT,
            callbacks.read(|_, card| card.vga.registers.sequencer.index()),
            callbacks.write(|_, card, value| card.vga.registers.sequencer.select(value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            SEQUENCER_DATA_PORT,
            callbacks.read(|_, card| card.vga.registers.sequencer.read()),
            callbacks.write(|context, card, value| card.write_sequencer_data(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            GRAPHICS_INDEX_PORT,
            callbacks.read(|_, card| card.vga.registers.graphics.index()),
            callbacks.write(|_, card, value| card.vga.registers.graphics.select(value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            GRAPHICS_DATA_PORT,
            callbacks.read(|_, card| card.vga.registers.graphics.read()),
            callbacks.write(|context, card, value| card.write_graphics_data(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_MASK_PORT,
            callbacks.read(|_, card| card.vga.registers.dac.mask),
            callbacks.write(|context, card, value| card.write_dac_mask(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_READ_ADDRESS_PORT,
            callbacks.read(|_, card| card.vga.registers.dac.state()),
            callbacks.write(|_, card, value| card.vga.registers.dac.select(value, true)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_WRITE_ADDRESS_PORT,
            callbacks.read(|_, card| card.vga.registers.dac.address()),
            callbacks.write(|_, card, value| card.vga.registers.dac.select(value, false)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_DATA_PORT,
            callbacks.read(|_, card| card.vga.registers.dac.read()),
            callbacks.write(|context, card, value| card.write_dac_data(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            FEATURE_CONTROL_READ_PORT,
            callbacks.read(|_, card| card.vga.registers.feature_control),
            |_, _| {},
        ),
        Mapping::byte(
            io,
            COMMON,
            MISC_OUTPUT_READ_PORT,
            callbacks.read(|_, card| card.vga.registers.miscellaneous),
            |_, _| {},
        ),
    ]
}

fn bind_dispi_ports(
    io: &mut DeviceConnection<'_>,
    callbacks: &DeviceCallbacks<DispiAdapter>,
) -> [Mapping; 2] {
    [
        Mapping::bind(
            io,
            dispi::INDEX_PORT,
            PortRead::word(callbacks.read(|_, card| card.dispi.index())),
            PortWrite::word(callbacks.write(|_, card, value| card.dispi.select(value))),
        ),
        Mapping::bind(
            io,
            dispi::DATA_PORT,
            PortRead::word(callbacks.read(|_, card| card.dispi.read())),
            PortWrite::word(
                callbacks.write(|context, card, value| card.write_dispi_data(context, value)),
            ),
        ),
    ]
}

fn crtc(
    io: &mut DeviceConnection<'_>,
    device: &Rc<RefCell<DispiAdapter>>,
    callbacks: &DeviceCallbacks<DispiAdapter>,
    index: u16,
    status: u16,
) -> Vec<Mapping> {
    let window = index..=index + 1;
    vec![
        Mapping::byte(
            io,
            window.clone(),
            index,
            callbacks.read(|_, card| card.vga.registers.crtc.index()),
            callbacks.write(|_, card, value| card.vga.registers.crtc.select(value)),
        ),
        Mapping::byte(
            io,
            window,
            index + 1,
            callbacks.read(|_, card| card.vga.registers.crtc.read()),
            callbacks.write(|context, card, value| card.write_crtc_data(context, value)),
        ),
        Mapping::bind(
            io,
            status,
            PortRead::low_byte(status::read(&device.borrow(), device)),
            PortWrite::low_byte(
                callbacks.write(|_, card, value| card.vga.registers.feature_control = value),
            ),
        ),
    ]
}

impl Io {
    /// Only a change to a group's decoding replaces its prepared operations.
    /// `old == None` initializes every group, including disabled addresses.
    /// Index selection never calls this method.
    pub fn update_decoding(
        &self,
        mut context: IoContext<'_>,
        old: Option<PortDecoding>,
        new: PortDecoding,
    ) {
        for (group, mappings) in [
            (PortGroup::Common, self.common.as_slice()),
            (PortGroup::Dispi, self.dispi.as_slice()),
            (PortGroup::Monochrome, self.mono.as_slice()),
            (PortGroup::Color, self.color.as_slice()),
        ] {
            let enabled = new.enables(group);
            if old.map(|mode| mode.enables(group)) != Some(enabled) {
                self.install_group(context.reborrow(), mappings, enabled);
            }
        }
    }

    fn install_group(&self, mut context: IoContext<'_>, mappings: &[Mapping], enabled: bool) {
        for mapping in mappings {
            mapping.binding.set_read(
                context.reborrow(),
                if enabled {
                    &mapping.active_read
                } else {
                    &self.disabled_read
                },
            );
            mapping.binding.set_write(
                context.reborrow(),
                if enabled {
                    &mapping.active_write
                } else {
                    &self.disabled_write
                },
            );
        }
    }
}
