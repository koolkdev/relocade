use std::ops::{Deref, DerefMut};

/// An exclusive borrow of backend state. Browser backends retain their store's
/// borrow guard here when lending state outside a callback.
pub struct StateAccess<'a, T>(Borrow<'a, T>);

enum Borrow<'a, T> {
    Direct(&'a mut T),
    #[cfg(target_arch = "wasm32")]
    Guard(std::cell::RefMut<'a, T>),
}

impl<'a, T> From<&'a mut T> for StateAccess<'a, T> {
    #[inline]
    fn from(state: &'a mut T) -> Self {
        Self(Borrow::Direct(state))
    }
}

#[cfg(target_arch = "wasm32")]
impl<'a, T> From<std::cell::RefMut<'a, T>> for StateAccess<'a, T> {
    fn from(state: std::cell::RefMut<'a, T>) -> Self {
        Self(Borrow::Guard(state))
    }
}

impl<T> Deref for StateAccess<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        match &self.0 {
            Borrow::Direct(state) => state,
            #[cfg(target_arch = "wasm32")]
            Borrow::Guard(state) => state,
        }
    }
}

impl<T> DerefMut for StateAccess<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        match &mut self.0 {
            Borrow::Direct(state) => state,
            #[cfg(target_arch = "wasm32")]
            Borrow::Guard(state) => state,
        }
    }
}
