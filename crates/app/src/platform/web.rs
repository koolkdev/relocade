use winit::{
    application::ApplicationHandler,
    error::{EventLoopError, OsError},
    event_loop::{ActiveEventLoop, EventLoop},
    platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys, WindowExtWebSys},
    window::{Window, WindowAttributes},
};

pub(super) fn run<T: 'static, A: ApplicationHandler<T> + 'static>(
    event_loop: EventLoop<T>,
    application: A,
) -> Result<(), EventLoopError> {
    event_loop.spawn_app(application);
    Ok(())
}

pub(super) fn create_window(
    event_loop: &ActiveEventLoop,
    attributes: WindowAttributes,
) -> Result<Window, OsError> {
    event_loop.create_window(attributes.with_append(true))
}

pub(super) fn detach_window(window: &Window) {
    if let Some(canvas) = window.canvas() {
        canvas.remove();
    }
}
