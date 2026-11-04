use super::*;
use futures_util::{
    future::{select, Either},
    stream::FuturesUnordered,
    FutureExt, StreamExt,
};
use relocade_wasm_runtime::worker::Executor;
use std::{future::poll_fn, pin::pin, task::Poll};

async fn turns() {
    for cancel in [false, true] {
        let (released, _release) = oneshot::channel();
        let signal = Signal::new(released);
        let (_host, endpoint) = wake_connection(signal.clone());
        let thread = Thread::new();
        let task = thread.run_resource(signal, move |signal| async move {
            let executor = Executor::new(endpoint.bind(signal));
            let (stop, stopped) = oneshot::channel::<()>();
            let mut stop = Some(stop);
            let root = async {
                // Cancelling one request must not cancel the other children.
                assert!(executor.yield_to_host().now_or_never().is_none());
                let children = FuturesUnordered::new();
                for value in [1, 2] {
                    let executor = &executor;
                    children.push(async move {
                        executor.yield_to_host().await;
                        value
                    });
                }
                let busy = poll_fn(|cx| {
                    if cancel {
                        if let Some(stop) = stop.take() {
                            stop.send(()).unwrap();
                        }
                    }
                    cx.waker().wake_by_ref();
                    Poll::<()>::Pending
                });
                // Continuously runnable siblings cannot prevent the host turn.
                match select(pin!(children.collect::<Vec<_>>()), pin!(busy)).await {
                    Either::Left((mut values, _)) => {
                        values.sort();
                        values
                    }
                    Either::Right(_) => unreachable!(),
                }
            };
            executor.run_until(stopped, root).await
        });
        assert_eq!(
            task.await.unwrap(),
            if cancel { None } else { Some(vec![1, 2]) }
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_turns_resume_nested_children_and_observe_session_cancellation() {
    pollster::block_on(turns());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_turns_resume_nested_children_and_observe_session_cancellation() {
    turns().await;
}
