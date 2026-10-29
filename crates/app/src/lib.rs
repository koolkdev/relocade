//! Launch a main-thread winit application on native platforms or the browser.
#![forbid(unsafe_code)]

pub use winit;

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
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut application = application;
        event_loop.run_app(&mut application)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(application);
        Ok(())
    }
}
