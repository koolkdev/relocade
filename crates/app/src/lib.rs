//! Launch a main-thread winit application on native platforms or the browser.
#![forbid(unsafe_code)]

pub use winit;

mod window;
pub use window::{Window, WindowState};

#[cfg(feature = "graphics")]
mod application;
#[cfg(feature = "graphics")]
pub mod graphics;
#[cfg(feature = "graphics")]
pub use application::{run_app, AppHost, PeripheralConnections};
#[cfg(feature = "graphics")]
pub use web_thread as worker;

#[cfg_attr(not(target_arch = "wasm32"), path = "platform/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "platform/web.rs")]
mod platform;

use winit::{
    application::ApplicationHandler,
    error::EventLoopError,
    event_loop::{EventLoop, EventLoopProxy},
};

/// Creates the event loop and application on the calling main thread.
///
/// The factory receives winit's proxy for application-defined notifications.
/// The application and factory need not be `Send`; window creation, event
/// handling and cleanup belong in the application's winit callbacks.
///
/// Native platforms run until the event loop exits. Browsers register callbacks
/// and return immediately; returning `Ok(())` there means startup, not completion.
/// This function creates no workers and does not configure rendering or execution.
pub fn run<T: 'static, A: ApplicationHandler<T> + 'static>(
    create: impl FnOnce(EventLoopProxy<T>) -> A,
) -> Result<(), EventLoopError> {
    let event_loop = EventLoop::<T>::with_user_event().build()?;
    let application = create(event_loop.create_proxy());
    platform::run(event_loop, application)
}
