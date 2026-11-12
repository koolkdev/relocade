//! Adapt device methods to concrete callbacks independently of port registration.

use crate::IoContext;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

/// Creates callbacks that briefly borrow a device. Retained callbacks capture a
/// weak reference, so a device can own its own port operations without a cycle.
/// Independent shared state, such as cached status, uses plain callbacks instead.
pub struct DeviceCallbacks<D>(Weak<RefCell<D>>);

impl<D: 'static> DeviceCallbacks<D> {
    pub fn new(device: &Rc<RefCell<D>>) -> Self {
        Self(Rc::downgrade(device))
    }

    pub fn read<T: 'static>(
        &self,
        read: impl Fn(IoContext<'_>, &mut D) -> T + 'static,
    ) -> impl Fn(IoContext<'_>) -> T + 'static {
        let device = self.0.clone();
        move |context| {
            read(
                context,
                &mut device.upgrade().expect("connected device").borrow_mut(),
            )
        }
    }

    pub fn write<T: 'static>(
        &self,
        write: impl Fn(IoContext<'_>, &mut D, T) + 'static,
    ) -> impl Fn(IoContext<'_>, T) + 'static {
        let device = self.0.clone();
        move |context, value| {
            write(
                context,
                &mut device.upgrade().expect("connected device").borrow_mut(),
                value,
            )
        }
    }
}
