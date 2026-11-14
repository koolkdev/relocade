//! ET4000 extensions to the concrete VGA operations on the same connection.
use super::{registers::*, Et4000Ax};
use crate::{
    effects::{CardEffects, Effects},
    ports::{Mapping, Ports, VgaPortAccess},
    registers::{crtc, sequencer, WriteKind},
    VgaCore,
};
use relocade_machine::{DeviceCallbacks, DeviceConnection, IoContext, PortRead, PortWrite};
use std::{cell::RefCell, rc::Rc};

impl Et4000Ax {
    pub(super) fn bind_ports(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) -> Ports {
        let mut ports = Ports::new(io, device);
        let callbacks = DeviceCallbacks::new(device);
        ports.add_common([
            Mapping::bind(
                io,
                HERCULES_CONTROL,
                PortRead::low_byte(|_| 0xff),
                PortWrite::low_byte(callbacks.write(|_, card, value| {
                    card.extensions.write_hercules(value);
                })),
            ),
            Mapping::byte(
                io,
                0x3c0..=0x3cf,
                SEGMENT_SELECT,
                callbacks.read(|_, card| card.extensions.read_segments()),
                callbacks.write(|context, card, value| {
                    card.with_effects(context, Effects::MEMORY, |card| {
                        card.extensions.write_segments(value);
                    });
                }),
            ),
        ]);
        for (address, color) in [(0x3b8, false), (0x3d8, true)] {
            let mapping = Mapping::bind(
                io,
                address,
                PortRead::low_byte(callbacks.read(move |_, card| card.extensions.read_mode(color))),
                PortWrite::low_byte(callbacks.write(|_, card, value| {
                    card.extensions.write_mode(value);
                })),
            );
            if color {
                ports.add_color(mapping);
            } else {
                ports.add_monochrome(mapping);
            }
        }
        ports.add_always(Mapping::byte(
            io,
            0x3c0..=0x3cf,
            VIDEO_ENABLE,
            callbacks.read(|_, card| {
                if card.extensions.uses_46e8() {
                    0xff
                } else {
                    card.vga.setup.read_control() >> 3
                }
            }),
            callbacks.write(|context, card, value| {
                if !card.extensions.uses_46e8() {
                    card.with_effects(context, Effects::IO, |card| {
                        card.vga.setup.write_control((value & 1) << 3);
                    });
                }
            }),
        ));
        // Rev E decodes only the low twelve address bits of 46E8h.
        let read = PortRead::low_byte(callbacks.read(|_, card| card.read_adapter_control()));
        let write = PortWrite::low_byte(callbacks.write(|context, card, value| {
            card.write_adapter_control(context, value);
        }));
        for high in 0..16 {
            if high != 4 {
                io.bind((high << 12) | 0x6e8, &read, &write);
            }
        }
        ports
    }
}

impl VgaPortAccess for Et4000Ax {
    fn vga(&self) -> &VgaCore {
        &self.vga
    }
    fn vga_mut(&mut self) -> &mut VgaCore {
        &mut self.vga
    }
    fn refresh_raster(&mut self) {
        Et4000Ax::refresh_raster(self);
    }

    fn legacy_effect(&self, kind: WriteKind) -> Effects {
        match kind {
            WriteKind::Access => Effects::NONE,
            // Display fields also describe the packed layout on this card.
            WriteKind::Display | WriteKind::Timing => Effects::CONFIGURATION,
        }
    }

    fn select_crtc(&mut self, value: u8) {
        self.vga.registers.crtc.select_raw(value & 0x3f);
    }

    fn read_crtc(&self) -> u8 {
        self.extensions
            .crtc(self.vga.registers.crtc.index())
            .unwrap_or_else(|| self.vga.registers.crtc.read())
    }

    fn read_sequencer(&self) -> u8 {
        self.extensions
            .sequencer(self.vga.registers.sequencer.index())
            .unwrap_or_else(|| self.vga.registers.sequencer.read())
    }

    fn read_attribute(&self) -> u8 {
        if self.vga.registers.attribute.address() & 0x1f == ATTRIBUTE_MISC {
            self.extensions.attribute
        } else {
            self.vga.registers.attribute.read()
        }
    }

    fn read_input_status_0(&self) -> u8 {
        // Both feature straps are open on this board; KEY exposes them.
        0x10 | if self.extensions.unlocked() { 0x60 } else { 0 }
    }

    fn read_adapter_control(&self) -> u8 {
        if self.extensions.uses_46e8() {
            self.vga.setup.read_control() & 8
        } else {
            0xff
        }
    }

    fn write_adapter_control(&mut self, context: IoContext<'_>, value: u8) {
        if self.extensions.uses_46e8() {
            self.with_effects(context, Effects::IO, |card| {
                card.vga.setup.write_control(value)
            });
        }
    }

    fn write_crtc_data(&mut self, context: IoContext<'_>, value: u8) {
        let index = self.vga.registers.crtc.index();
        let effects = match index {
            EXTENDED_START => Effects::IMAGE,
            COMPATIBILITY => Effects::CONFIGURATION_AND_IO,
            OVERFLOW => Effects::CONFIGURATION,
            CONFIGURATION | 0x37 => Effects::CONFIGURATION_AND_MEMORY,
            0x32 => Effects::NONE,
            _ => self.legacy_effect(self.vga.registers.crtc.write_effect()),
        };
        self.with_effects(context, effects, |card| {
            if index >= 0x32 {
                let protected = card.vga.registers.crtc.registers.values
                    [usize::from(crtc::VERTICAL_RETRACE_END)]
                    & 0x80
                    != 0;
                card.extensions.write_crtc(index, value, protected);
            } else {
                card.vga.registers.crtc.write(value);
            }
        });
    }

    fn write_sequencer_data(&mut self, context: IoContext<'_>, value: u8) {
        let index = self.vga.registers.sequencer.index();
        let mut effects = if index >= 6 {
            Effects::CONFIGURATION
        } else {
            self.legacy_effect(self.vga.registers.sequencer.write_effect())
        };
        effects.memory_decode = matches!(index, sequencer::MAP_MASK | sequencer::MEMORY_MODE);
        self.with_effects(context, effects, |card| {
            if index >= 6 {
                card.extensions.write_sequencer(index, value);
            } else {
                card.vga.write_sequencer(value);
            }
            if index == sequencer::RESET && value & 2 == 0 {
                card.extensions.synchronous_reset();
            }
        });
    }

    fn write_attribute(&mut self, context: IoContext<'_>, value: u8) {
        let effects = if self.vga.registers.attribute.data_index() == Some(ATTRIBUTE_MISC) {
            Effects::CONFIGURATION
        } else {
            self.legacy_effect(self.vga.registers.attribute.write_effect())
        };
        self.with_effects(context, effects, |card| {
            let extensions = &mut card.extensions;
            card.vga
                .registers
                .attribute
                .write_with(value, |index, value| {
                    extensions.write_attribute(index, value)
                });
        });
    }
}
