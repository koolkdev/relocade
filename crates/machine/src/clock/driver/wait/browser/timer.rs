//! Own a worker-local deadline and cancel it when its Rust waiter is dropped.

use js_sys::Promise;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_time::Instant;

#[wasm_bindgen(module = "/src/clock/driver/wait/browser/timer.js")]
extern "C" {
    type DeadlineTimer;
    #[wasm_bindgen(constructor)]
    fn new() -> DeadlineTimer;
    #[wasm_bindgen(method)]
    fn start(this: &DeadlineTimer, milliseconds: f64) -> Promise;
    #[wasm_bindgen(method)]
    fn cancel(this: &DeadlineTimer);
    #[wasm_bindgen(method)]
    fn close(this: &DeadlineTimer);
}

pub(super) struct Timer(DeadlineTimer);

impl Timer {
    pub fn new() -> Self {
        Self(DeadlineTimer::new())
    }

    pub fn start(&mut self, deadline: Instant) -> Pending<'_> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        Pending {
            timer: &self.0,
            future: JsFuture::from(self.0.start(remaining.as_secs_f64() * 1000.0)),
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.0.close();
    }
}

pub(super) struct Pending<'a> {
    timer: &'a DeadlineTimer,
    future: JsFuture,
}

impl Future for Pending<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.future).poll(cx).map(|result| {
            result.expect("worker deadline timer failed");
        })
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.timer.cancel();
    }
}

#[cfg(test)]
mod tests;
