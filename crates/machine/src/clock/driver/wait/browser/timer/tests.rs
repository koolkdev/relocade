use super::DeadlineTimer;
use crate::worker::Thread;
use std::time::Duration;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelling_queued_and_armed_timers_settles_their_promises() {
    let thread = Thread::new();
    thread
        .run_send((), |()| async {
            let timer = DeadlineTimer::new();
            let queued = JsFuture::from(timer.start(3_600_000.0));
            timer.cancel();
            queued.await.unwrap();

            let armed = JsFuture::from(timer.start(3_600_000.0));
            futures_timer::Delay::new(Duration::from_millis(10)).await;
            timer.cancel();
            armed.await.unwrap();

            // Old queued work cannot prevent a replacement deadline completing.
            JsFuture::from(timer.start(1.0)).await.unwrap();
            let closing = JsFuture::from(timer.start(3_600_000.0));
            timer.close();
            closing.await.unwrap();
        })
        .await
        .unwrap();
}
