//! Launch a main-thread winit application on native platforms or the browser.
#![forbid(unsafe_code)]

pub use winit;

#[cfg_attr(not(target_arch = "wasm32"), path = "platform/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "platform/web.rs")]
mod platform;

use std::future::Future;
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
/// Returns a notification to await before asynchronous shutdown work:
/// `let exited = run(create)?; exited.await;`.
///
/// Native desktops run until the event loop exits, then return a ready notification.
/// Browsers register callbacks and return immediately; the notification completes
/// after the application's `exiting` callback returns. Await it from a browser
/// executor such as `wasm_bindgen_futures::spawn_local`, without blocking main.
/// Dropping the notification does not stop the application.
/// This function creates no workers and does not configure rendering or execution.
pub fn run<T: 'static, A: ApplicationHandler<T> + 'static>(
    create: impl FnOnce(EventLoopProxy<T>) -> A,
) -> Result<impl Future<Output = ()> + 'static, EventLoopError> {
    let event_loop = EventLoop::<T>::with_user_event().build()?;
    let application = create(event_loop.create_proxy());
    platform::run(event_loop, application)
}
