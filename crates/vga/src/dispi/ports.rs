//! Additional DISPI word ports composed with the common VGA interface.
use crate::{
    dispi,
    effects::{CardEffects, Effects},
    ports::{Mapping, Ports, VgaPortAccess},
    registers::WriteKind,
    DispiAdapter, VgaCore,
};
use relocade_machine::{DeviceCallbacks, DeviceConnection, PortRead, PortWrite};
use std::{cell::RefCell, rc::Rc};

impl DispiAdapter {
    pub(super) fn bind_ports(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) -> Ports {
        let mut ports = Ports::new(io, device);
        ports.add_common(bind_dispi_ports(io, &DeviceCallbacks::new(device)));
        ports.update_decoding(io.context(), None, device.borrow().port_decoding());
        ports
    }
}

impl VgaPortAccess for DispiAdapter {
    fn vga(&self) -> &VgaCore {
        &self.vga
    }
    fn vga_mut(&mut self) -> &mut VgaCore {
        &mut self.vga
    }
    fn refresh_raster(&mut self) {
        DispiAdapter::refresh_raster(self);
    }
    fn legacy_effect(&self, kind: WriteKind) -> Effects {
        if self.dispi.display_layout().is_some() {
            Effects::NONE
        } else {
            Effects::vga(kind)
        }
    }
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
