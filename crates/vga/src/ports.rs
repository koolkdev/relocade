//! Connection-time wiring and changes to physical address decoding.

mod access;
pub(crate) use access::VgaPortAccess;

use crate::registers::*;
use relocade_machine::{
    DeviceCallbacks, DeviceConnection, IoContext, PortBinding, PortRange, PortRead, PortWrite,
};
use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

const COMMON: RangeInclusive<u16> = ATTRIBUTE_ADDRESS_PORT..=GRAPHICS_DATA_PORT;

/// Physical VGA address groups enabled by setup and Miscellaneous Output.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortDecoding {
    Disabled,
    Monochrome,
    Color,
}

#[derive(Clone, Copy)]
enum PortGroup {
    Common,
    Monochrome,
    Color,
}

impl PortDecoding {
    fn enables(self, group: PortGroup) -> bool {
        match self {
            Self::Disabled => false,
            Self::Monochrome => matches!(group, PortGroup::Common | PortGroup::Monochrome),
            Self::Color => matches!(group, PortGroup::Common | PortGroup::Color),
        }
    }
}

/// Retained only for decoding changes. Ordinary accesses invoke the installed
/// operations directly, without consulting this companion or its mappings.
pub(crate) struct Ports {
    common: Vec<Mapping>,
    mono: Vec<Mapping>,
    color: Vec<Mapping>,
    always: Vec<Mapping>,
    disabled_read: PortRead,
    disabled_write: PortWrite,
}

pub(crate) struct Mapping {
    binding: PortBinding,
    active_read: PortRead,
    active_write: PortWrite,
}

impl Mapping {
    pub(crate) fn bind(
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

    pub(crate) fn byte(
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

impl Ports {
    pub fn new<Card: VgaPortAccess>(
        io: &mut DeviceConnection<'_>,
        device: &Rc<RefCell<Card>>,
    ) -> Self {
        let callbacks = DeviceCallbacks::new(device);
        let common = bind_common_vga_ports(io, &callbacks);
        // Setup remains accessible while the adapter's other addresses are off.
        io.bind(
            OPTION_SELECT_PORT,
            &PortRead::low_byte(callbacks.read(|_, card| card.vga().setup.read_option())),
            &PortWrite::low_byte(
                callbacks.write(|context, card, value| card.write_option_select(context, value)),
            ),
        );
        io.bind(
            ADAPTER_CONTROL_PORT,
            &PortRead::low_byte(callbacks.read(|_, card| card.read_adapter_control())),
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
        Self {
            common,
            mono,
            color,
            always: Vec::new(),
            // These consume the entire transfer, unlike enabled-window holes.
            disabled_read: PortRead::new(|_, _, _| u32::MAX),
            disabled_write: PortWrite::new(|_, _, _, _| {}),
        }
    }

    pub fn add_common(&mut self, mappings: impl IntoIterator<Item = Mapping>) {
        self.common.extend(mappings);
    }

    pub fn add_monochrome(&mut self, mapping: Mapping) {
        self.mono.push(mapping);
    }
    pub fn add_color(&mut self, mapping: Mapping) {
        self.color.push(mapping);
    }
    pub fn add_always(&mut self, mapping: Mapping) {
        self.always.push(mapping);
    }
}

impl PortDecoding {
    pub fn vga(vga: &crate::VgaCore) -> Self {
        if !vga.setup.enabled() {
            Self::Disabled
        } else if vga.registers.miscellaneous & 1 != 0 {
            Self::Color
        } else {
            Self::Monochrome
        }
    }
}

fn bind_common_vga_ports<Card: VgaPortAccess>(
    io: &mut DeviceConnection<'_>,
    callbacks: &DeviceCallbacks<Card>,
) -> Vec<Mapping> {
    // Enabled holes must continue wide transfers to later implemented ports.
    vec![
        Mapping::byte(io, COMMON, COMMON, |_| 0xff, |_, _| {}),
        Mapping::byte(
            io,
            COMMON,
            ATTRIBUTE_ADDRESS_PORT,
            callbacks.read(|_, card| card.vga().registers.attribute.address()),
            callbacks.write(|context, card, value| card.write_attribute(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            ATTRIBUTE_DATA_READ_PORT,
            callbacks.read(|_, card| card.read_attribute()),
            |_, _| {},
        ),
        Mapping::byte(
            io,
            COMMON,
            MISC_OUTPUT_WRITE_PORT,
            callbacks.read(|_, card| card.read_input_status_0()),
            callbacks.write(|context, card, value| card.write_misc_output(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            SEQUENCER_INDEX_PORT,
            callbacks.read(|_, card| card.vga().registers.sequencer.index()),
            callbacks.write(|_, card, value| card.select_sequencer(value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            SEQUENCER_DATA_PORT,
            callbacks.read(|_, card| card.read_sequencer()),
            callbacks.write(|context, card, value| card.write_sequencer_data(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            GRAPHICS_INDEX_PORT,
            callbacks.read(|_, card| card.vga().registers.graphics.index()),
            callbacks.write(|_, card, value| card.select_graphics(value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            GRAPHICS_DATA_PORT,
            callbacks.read(|_, card| card.read_graphics()),
            callbacks.write(|context, card, value| card.write_graphics_data(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_MASK_PORT,
            callbacks.read(|_, card| card.read_dac_mask()),
            callbacks.write(|context, card, value| card.write_dac_mask(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_READ_ADDRESS_PORT,
            callbacks.read(|_, card| card.access_dac_palette().state()),
            callbacks.write(|_, card, value| card.access_dac_palette().select(value, true)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_WRITE_ADDRESS_PORT,
            callbacks.read(|_, card| card.access_dac_palette().address()),
            callbacks.write(|_, card, value| card.access_dac_palette().select(value, false)),
        ),
        Mapping::byte(
            io,
            COMMON,
            DAC_DATA_PORT,
            callbacks.read(|_, card| card.access_dac_palette().read()),
            callbacks.write(|context, card, value| card.write_dac_data(context, value)),
        ),
        Mapping::byte(
            io,
            COMMON,
            FEATURE_CONTROL_READ_PORT,
            callbacks.read(|_, card| card.vga().registers.feature_control),
            |_, _| {},
        ),
        Mapping::byte(
            io,
            COMMON,
            MISC_OUTPUT_READ_PORT,
            callbacks.read(|_, card| card.vga().registers.miscellaneous),
            |_, _| {},
        ),
    ]
}

fn crtc<Card: VgaPortAccess>(
    io: &mut DeviceConnection<'_>,
    device: &Rc<RefCell<Card>>,
    callbacks: &DeviceCallbacks<Card>,
    index: u16,
    status: u16,
) -> Vec<Mapping> {
    let read_status = {
        let mut card = device.borrow_mut();
        let read =
            crate::output::status::read(&card.output().raster.status, device, Card::refresh_raster);
        card.vga().registers.attribute.handle_read(read)
    };
    let window = index..=index + 1;
    vec![
        Mapping::byte(
            io,
            window.clone(),
            index,
            callbacks.read(|_, card| card.vga().registers.crtc.index()),
            callbacks.write(|_, card, value| card.select_crtc(value)),
        ),
        Mapping::byte(
            io,
            window,
            index + 1,
            callbacks.read(|_, card| card.read_crtc()),
            callbacks.write(|context, card, value| card.write_crtc_data(context, value)),
        ),
        Mapping::bind(
            io,
            status,
            PortRead::low_byte(move |_| Card::input_status_1(read_status())),
            PortWrite::low_byte(
                callbacks.write(|_, card, value| card.vga_mut().registers.feature_control = value),
            ),
        ),
    ]
}

impl Ports {
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
            (PortGroup::Monochrome, self.mono.as_slice()),
            (PortGroup::Color, self.color.as_slice()),
        ] {
            let enabled = new.enables(group);
            if old.map(|mode| mode.enables(group)) != Some(enabled) {
                self.install_group(context.reborrow(), mappings, enabled);
            }
        }
        // An enable register inside the common window must survive its holes
        // being reinstalled when the rest of the adapter is turned off.
        if old.map(|mode| mode.enables(PortGroup::Common)) != Some(new.enables(PortGroup::Common)) {
            self.install_group(context, &self.always, true);
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
