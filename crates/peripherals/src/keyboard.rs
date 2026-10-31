//! Keyboard events crossing from a host window to an execution-owned device.
//! Physical keys and host text are separate: the device chooses guest encoding
//! and scan codes. The transport is an ordinary asynchronous Rust channel.

use futures_channel::mpsc;

pub type KeyboardInput = mpsc::UnboundedSender<KeyboardEvent>;
pub type KeyboardEvents = mpsc::UnboundedReceiver<KeyboardEvent>;

pub fn channel() -> (KeyboardInput, KeyboardEvents) {
    mpsc::unbounded()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardEvent {
    Key {
        code: KeyCode,
        text: Option<String>,
        pressed: bool,
        repeat: bool,
    },
    Modifiers(Modifiers),
    /// Release transient state when the host loses keyboard focus.
    FocusLost,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

/// Physical positions of the common keyboard keys, independent of text layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Backquote,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Space,
    Enter,
    Escape,
    Backspace,
    Tab,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    SuperLeft,
    SuperRight,
    CapsLock,
    NumLock,
    ScrollLock,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadDecimal,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadEnter,
}
