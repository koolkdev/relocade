use futures_channel::oneshot;
use std::future::Future;
use winit::{
    application::ApplicationHandler,
    error::EventLoopError,
    event::{DeviceEvent, DeviceId, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    platform::web::EventLoopExtWebSys,
    window::WindowId,
};

pub(super) fn run<T: 'static, A: ApplicationHandler<T> + 'static>(
    event_loop: EventLoop<T>,
    application: A,
) -> Result<impl Future<Output = ()> + 'static, EventLoopError> {
    let (exited, notification) = oneshot::channel();
    event_loop.spawn_app(NotifyExit {
        application,
        exited: Some(exited),
    });
    Ok(async move {
        notification.await.expect("event loop dropped before exit");
    })
}

// Preserve all winit callbacks while observing the end of the application's cleanup.
struct NotifyExit<A> {
    application: A,
    exited: Option<oneshot::Sender<()>>,
}

impl<T: 'static, A: ApplicationHandler<T>> ApplicationHandler<T> for NotifyExit<A> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.application.new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.application.resumed(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: T) {
        self.application.user_event(event_loop, event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.application.window_event(event_loop, id, event);
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.application.device_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.application.about_to_wait(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.application.suspended(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.application.exiting(event_loop);
        if let Some(exited) = self.exited.take() {
            let _ = exited.send(());
        }
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.application.memory_warning(event_loop);
    }
}
