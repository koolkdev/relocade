use futures_channel::oneshot;
use futures_util::FutureExt;
use relocade_workers::{
    channel, CancellableWorker, ReceiverEndpoint, ResourceTransfer, Thread, ThreadExt,
    WorkerResource,
};
use std::{rc::Rc, sync::Arc};

// A resource need not be Send, including on native targets. Each owner rebuilds
// its own local wrapper around the posted value.
struct Local(Rc<u32>);

impl WorkerResource for Local {
    type Posted = u32;

    fn prepare(self) -> ResourceTransfer<u32, impl FnOnce(u32) -> Self + Send> {
        ResourceTransfer::new(*self.0, |value| Self(Rc::new(value)))
    }
}

#[derive(WorkerResource)]
#[worker(crate = relocade_workers)]
struct Configured {
    retained: Arc<Vec<u8>>,
    #[worker(resource)]
    value: Local,
}

#[derive(WorkerResource)]
#[worker(crate = relocade_workers)]
struct Inbox {
    #[worker(resource)]
    endpoint: ReceiverEndpoint<Configured>,
    retained: Arc<Vec<u8>>,
}

async fn repeated_delivery() {
    let (sender, endpoint) = channel::<Configured>().unwrap();
    let retained = Arc::new(vec![17, 29]);
    let (ready, started) = oneshot::channel();
    let receiver_worker = Thread::new();
    let receive = receiver_worker.run_resource(
        Inbox {
            endpoint,
            retained: retained.clone(),
        },
        move |inbox| async move {
            let mut receiver = inbox.endpoint.bind();
            assert!(receiver.recv().now_or_never().is_none());
            ready.send(()).unwrap();
            let mut values = Vec::new();
            while let Some(resource) = receiver.recv().await {
                let resource = resource.unwrap();
                assert!(Arc::ptr_eq(&resource.retained, &inbox.retained));
                values.push(*resource.value.0);
            }
            values
        },
    );
    started.await.unwrap();
    let sender_worker = Thread::new();
    let send = sender_worker.run_resource(sender, move |mut sender| async move {
        for value in [42, 73] {
            sender
                .send(Configured {
                    retained: retained.clone(),
                    value: Local(Rc::new(value)),
                })
                .unwrap();
        }
        // Closing must drain both objects, even before their browser delivery.
    });
    assert_eq!(receive.await.unwrap(), [42, 73]);
    send.await.unwrap();

    let (mut sender, endpoint) = channel::<Local>().unwrap();
    drop(endpoint);
    assert!(sender.send(Local(Rc::new(99))).is_err());
}

async fn attach_to_running_worker() {
    let (sender, endpoint) = relocade_workers::oneshot::channel::<Configured>();
    let retained = Arc::new(vec![17, 29]);
    let expected = retained.clone();
    let (started, ready) = oneshot::channel();
    let worker = CancellableWorker::spawn(|thread, _stopped| {
        thread.run_send((), move |()| async move {
            let receiver = endpoint.bind().unwrap();
            started.send(()).unwrap();
            let resource = receiver.receive().await.unwrap();
            assert!(Arc::ptr_eq(&resource.retained, &expected));
            *resource.value.0
        })
    });
    ready.await.unwrap();
    worker
        .send_resource(
            sender,
            Configured {
                retained,
                value: Local(Rc::new(42)),
            },
        )
        .await
        .unwrap();
    assert_eq!(worker.join().await.unwrap(), 42);

    // A dispatch to the wrong owner must also resolve the waiting receiver.
    let owner = Thread::new();
    let other = Thread::new();
    let (sender, endpoint) = relocade_workers::oneshot::channel::<Local>();
    let received = owner.run_send((), move |()| async move {
        endpoint.bind().unwrap().receive().await.is_err()
    });
    assert!(sender.send(&other, Local(Rc::new(73))).await.is_err());
    assert!(received.await.unwrap());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn channels_reconstruct_complete_objects_and_drain_before_close() {
    pollster::block_on(repeated_delivery());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn channels_reconstruct_complete_objects_and_drain_before_close() {
    repeated_delivery().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resources_attach_after_worker_startup() {
    pollster::block_on(attach_to_running_worker());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn resources_attach_after_worker_startup() {
    attach_to_running_worker().await;
}

#[cfg(target_arch = "wasm32")]
mod posting_errors {
    use super::*;
    use relocade_workers::{AsJs, Post};
    use wasm_bindgen::JsValue;

    struct Rejected;

    impl AsJs for Rejected {
        fn to_js(&self) -> Result<JsValue, JsValue> {
            Err(JsValue::from_str("rejected resource"))
        }

        fn from_js(_: JsValue) -> Result<Self, JsValue> {
            unreachable!("the resource is never posted")
        }
    }

    impl Post for Rejected {}

    #[derive(WorkerResource)]
    #[worker(crate = relocade_workers)]
    struct Resource(#[worker(post)] Rejected);

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn failed_handoff_completes_the_waiting_receiver() {
        let thread = Thread::new();
        let (sender, endpoint) = relocade_workers::oneshot::channel::<Resource>();
        let received = thread.run_send((), move |()| async move {
            endpoint.bind().unwrap().receive().await.is_err()
        });
        assert!(sender.send(&thread, Resource(Rejected)).await.is_err());
        assert!(received.await.unwrap());
    }
}
