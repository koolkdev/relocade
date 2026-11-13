//! The installed video card and its display capabilities.
use relocade_bios::video::VideoFirmware;
use relocade_vga::{DispiAdapter, DisplaySnapshot, FramebufferAccess};
use std::{cell::RefCell, rc::Rc};

pub enum VideoCard {
    Dispi(Rc<RefCell<DispiAdapter>>),
}

impl VideoCard {
    /// Snapshot queue for one graphics consumer on another worker.
    pub fn frames(&self) -> async_channel::Receiver<DisplaySnapshot> {
        match self {
            Self::Dispi(card) => card.borrow().frames(),
        }
    }

    /// Native drawing access on the execution owner. End borrows before polling.
    pub fn framebuffer_access(&self) -> Rc<RefCell<dyn FramebufferAccess>> {
        match self {
            Self::Dispi(card) => card.clone(),
        }
    }

    pub(crate) fn firmware(&self) -> VideoFirmware {
        match self {
            Self::Dispi(card) => VideoFirmware::Dispi(card.clone()),
        }
    }
}
