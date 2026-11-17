//! Owned relative mouse input for execution-side adapters.

use futures_channel::mpsc;
pub use winit::event::{ElementState, MouseButton};

pub type MouseInput = mpsc::UnboundedSender<MouseEvent>;
pub type MouseEvents = mpsc::UnboundedReceiver<MouseEvent>;

pub fn channel() -> (MouseInput, MouseEvents) {
    mpsc::unbounded()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseEvent {
    /// Relative host units: positive X is right and positive Y is down.
    /// Adapters retain fractional movement and choose guest scaling.
    Motion { delta_x: f64, delta_y: f64 },
    Button {
        button: MouseButton,
        state: ElementState,
    },
    /// Release held buttons when focus or pointer capture is lost.
    FocusLost,
}
