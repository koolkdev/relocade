//! Owned keyboard events passed from the application to an execution-side adapter.
//! Key identity and modifiers use winit's types; each adapter chooses guest text
//! encoding, scan codes or messages. Delivery uses an ordinary Rust channel.

use futures_channel::mpsc;
pub use winit::{
    event::ElementState,
    keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NativeKey, PhysicalKey},
};

pub type KeyboardInput = mpsc::UnboundedSender<KeyboardEvent>;
pub type KeyboardEvents = mpsc::UnboundedReceiver<KeyboardEvent>;

pub fn channel() -> (KeyboardInput, KeyboardEvents) {
    mpsc::unbounded()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardEvent {
    Key {
        physical_key: PhysicalKey,
        logical_key: Key,
        location: KeyLocation,
        text: Option<String>,
        state: ElementState,
        repeat: bool,
    },
    Modifiers(ModifiersState),
    /// Release transient state when the application loses keyboard focus.
    FocusLost,
}
