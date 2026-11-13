use relocade_adlib::{Adlib, ADDRESS_STATUS_PORT, DATA_PORT};
use relocade_machine::{Clock, IoBus};
use std::{cell::RefCell, rc::Rc};

pub fn hardware(clock: Clock) -> (IoBus, Rc<RefCell<Adlib>>) {
    let adlib = Rc::new(RefCell::new(Adlib::new(clock)));
    let io = IoBus::default();
    io.connect(adlib.clone(), |io| Adlib::configure_io(io, &adlib));
    (io, adlib)
}

pub fn write(io: &IoBus, register: u8, value: u8) {
    io.write_port(ADDRESS_STATUS_PORT, 1, register.into());
    io.write_port(DATA_PORT, 1, value.into());
}
