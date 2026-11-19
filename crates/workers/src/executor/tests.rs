use super::*;
use crate::wake_connection;
use futures_util::{task::noop_waker_ref, FutureExt};
use std::future::pending;

#[derive(Default, Clone)]
struct Signal(Arc<AtomicBool>);

impl WakeTarget for Signal {
    fn wake(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn acknowledge(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn waking_during_a_turn_joins_the_tail_once() {
    let signal = Signal::default();
    let (_host, endpoint) = wake_connection(signal.clone());
    let executor = Executor::new(endpoint.bind(signal.clone()));
    let device = executor.task(0);
    let machine = executor.task(1);
    machine.wake_by_ref();
    machine.wake_by_ref();

    let mut turns = Vec::new();
    let run = executor.drive(pending::<()>(), |id, cx| {
        assert!(!signal.0.load(Ordering::SeqCst), "new turn begins clear");
        turns.push(id);
        match turns.as_slice() {
            [1] => {
                assert!(!executor.has_ready());
                // Guest I/O reschedules the machine before end-of-turn clock
                // publication wakes a due device. The device must run first.
                cx.waker().wake_by_ref();
                cx.waker().wake_by_ref();
                assert!(executor.has_ready());
                assert!(signal.0.load(Ordering::SeqCst));
                device.wake_by_ref();
            }
            [1, 0] => {
                assert!(executor.has_ready());
                // An event for an already queued task still interrupts the
                // current synchronous turn without adding a second queue entry.
                machine.wake_by_ref();
                assert!(signal.0.load(Ordering::SeqCst));
            }
            [1, 0, 1] => {
                assert!(!executor.has_ready());
                return Poll::Ready(());
            }
            _ => panic!("unexpected FIFO order {turns:?}"),
        }
        Poll::Pending
    });
    assert_eq!(run.now_or_never(), Some(Some(())));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cancelling_a_drive_retires_escaped_task_wakers() {
    let signal = Signal::default();
    let (_host, endpoint) = wake_connection(signal.clone());
    let executor = Executor::new(endpoint.bind(signal));
    let task = executor.task(0);
    task.wake_by_ref();
    {
        let mut run = pin!(executor.drive(pending::<()>(), |_, _| Poll::<()>::Pending));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(noop_waker_ref()))
            .is_pending());
    }
    task.wake_by_ref();
    assert!(!executor.has_ready());

    executor.task(1).wake();
    let next = executor.drive(pending::<()>(), |id, _| Poll::Ready(id));
    assert_eq!(next.now_or_never(), Some(Some(1)));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn an_idle_queue_parks_without_a_host_retry_timer() {
    use futures_channel::oneshot;
    use std::rc::Rc;

    let signal = Signal::default();
    let (_host, endpoint) = wake_connection(signal.clone());
    let timers = Rc::new(RefCell::new(Vec::new()));
    let arm = timers.clone();
    let executor =
        Executor::new(endpoint.bind(signal)).with_host_timer(Duration::from_millis(1), move |_| {
            let (fire, wait) = oneshot::channel::<()>();
            arm.borrow_mut().push(fire);
            async move { wait.await.unwrap() }
        });
    let (complete, operation) = oneshot::channel::<()>();
    let demand = executor.host_turns();
    let mut operation = pin!(demand.wait(operation));
    executor.task(0).wake();
    let mut drive = pin!(executor.drive(pending::<()>(), |id, cx| match id {
        0 => operation.as_mut().poll(cx),
        _ => {
            assert!(!timers.borrow().last().unwrap().is_canceled());
            Poll::Pending
        }
    }));

    let mut cx = Context::from_waker(noop_waker_ref());
    assert!(drive.as_mut().poll(&mut cx).is_pending());
    assert!(timers.borrow().is_empty(), "no other task needs to run");

    executor.task(1).wake();
    assert!(drive.as_mut().poll(&mut cx).is_pending());
    assert!(timers.borrow()[0].is_canceled(), "worker is parked again");

    complete.send(()).unwrap();
    assert_eq!(drive.as_mut().poll(&mut cx), Poll::Ready(Some(Ok(()))));
}
