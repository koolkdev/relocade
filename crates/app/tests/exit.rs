#![cfg(target_arch = "wasm32")]

use futures_channel::oneshot;
use relocade_app::winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::ActiveEventLoop,
    window::WindowId,
};
use std::{
    cell::RefCell,
    future::Future,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct ExitWake(AtomicBool);

impl Wake for ExitWake {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct App {
    callbacks: Rc<RefCell<Vec<&'static str>>>,
    exit_wake: Arc<ExitWake>,
    closed: Option<oneshot::Sender<()>>,
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _: &ActiveEventLoop, _: StartCause) {
        self.callbacks.borrow_mut().push("new_events");
    }

    fn resumed(&mut self, _: &ActiveEventLoop) {
        self.callbacks.borrow_mut().push("resumed");
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        self.callbacks.borrow_mut().push("user_event");
        event_loop.exit();
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        // The launcher must wake the waiter after this callback, not before it.
        assert!(!self.exit_wake.0.load(Ordering::Relaxed));
        self.callbacks.borrow_mut().push("exiting");
        self.closed.take().unwrap().send(()).unwrap();
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn exit_notification_follows_cleanup_and_can_be_discarded() {
    for discard_notification in [false, true] {
        let callbacks = Rc::new(RefCell::new(Vec::new()));
        let exit_wake = Arc::new(ExitWake::default());
        let (closed, closure) = oneshot::channel();
        let mut proxy = None;
        let mut exited = Box::pin(
            relocade_app::run(|events| {
                proxy = Some(events);
                App {
                    callbacks: callbacks.clone(),
                    exit_wake: exit_wake.clone(),
                    closed: Some(closed),
                }
            })
            .unwrap(),
        );
        let waker = Waker::from(exit_wake.clone());
        assert!(exited
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending());

        if discard_notification {
            drop(exited);
            assert!(!callbacks.borrow().contains(&"exiting"));
            proxy.unwrap().send_event(()).unwrap();
            closure.await.unwrap();
        } else {
            proxy.unwrap().send_event(()).unwrap();
            closure.await.unwrap();
            assert!(exit_wake.0.load(Ordering::Relaxed));
            exited.await;
        }
        let callbacks = callbacks.borrow();
        assert_eq!(callbacks.first(), Some(&"new_events"));
        assert!(callbacks.contains(&"resumed"));
        assert!(callbacks.contains(&"user_event"));
        assert_eq!(callbacks.last(), Some(&"exiting"));
    }
}
