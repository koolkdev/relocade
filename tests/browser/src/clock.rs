use futures_util::future::join;
use relocade_machine::{timer_service, worker::Thread, Clock};
use std::{
    future::{poll_fn, Future},
    pin::pin,
    task::Poll,
    time::Duration,
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn timer_workers() -> Result<(), String> {
    let (clock, timers) = timer_service();
    let worker = Thread::new();
    let execution = worker.run_send((), move |()| async move {
        let deadline = clock.now() + Duration::from_millis(10);
        let mut sleep = pin!(clock.wait_until(deadline));
        let mut pending = 0;
        // Continuously ready Rust work never returns to this worker's browser
        // event loop. The host's timer must still deliver its channel completion.
        poll_fn(|cx| {
            if sleep.as_mut().poll(cx).is_ready() {
                return Poll::Ready(());
            }
            pending += 1;
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await;
        assert!(pending > 0);
        assert!(clock.now() >= deadline);
    });
    let ((), result) = join(timers.run(), execution).await;
    result.map_err(|error| error.to_string())
}
