use super::*;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Default)]
struct Interrupt(Arc<AtomicBool>);

impl WakeTarget for Interrupt {
    fn wake(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn acknowledge(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn acknowledging_a_worker_turn_preserves_independent_wake_reasons() {
    let reasons = Arc::new(WakeReasons::default());
    let interrupt = Interrupt::default();
    let (_host, endpoint) = crate::wake_connection(interrupt.clone());
    let worker = WorkerWake::with_reasons(endpoint.bind(interrupt.clone()), reasons.clone());
    let first = 1;
    let second = 2;

    reasons.record(first);
    worker.waker().wake_by_ref();
    worker.poll(|cx| {
        assert!(interrupt.0.load(Ordering::SeqCst));
        reasons.record(second);
        reasons.record(second);
        assert_eq!(reasons.take(first), first);
        cx.waker().wake_by_ref();
        cx.waker().wake_by_ref();
    });
    assert!(
        interrupt.0.load(Ordering::SeqCst),
        "a later wake needs a checkpoint"
    );

    worker.poll(|_| {
        assert!(reasons.pending(second));
        assert_eq!(reasons.take(first | second), second);
    });
    assert!(!interrupt.0.load(Ordering::SeqCst));
    assert!(!reasons.pending(first | second));
}
