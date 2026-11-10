use futures_channel::oneshot;
use futures_util::FutureExt;
use relocade_workers::{channel, ResourceTransfer, Worker, WorkerResource};
use std::{rc::Rc, sync::Arc};

// Reconstruction can create a local wrapper even on native workers.
struct Local(Rc<u32>);

impl WorkerResource for Local {
    type Posted = u32;

    fn prepare(self) -> ResourceTransfer<u32, impl FnOnce(u32) -> Self + Send> {
        ResourceTransfer::new(*self.0, |value| Self(Rc::new(value)))
    }
}

#[derive(WorkerResource)]
#[worker(crate = relocade_workers)]
struct Object<R> {
    retained: Arc<Vec<u8>>,
    #[worker(resource)]
    first: R,
    #[worker(resource)]
    second: R,
    #[worker(resource)]
    third: R,
}

async fn repeated_delivery() {
    let retained = Arc::new(vec![17, 29]);
    let (ready, started) = oneshot::channel();
    let receiver_worker = Worker::new();
    receiver_worker.run(|| async {}).await.unwrap();
    let (sender, mut receiver) = receiver_worker.channel::<Object<Local>>().await.unwrap();
    let received_retained = retained.clone();
    let receive = receiver_worker.run(move || async move {
        assert!(receiver.recv().now_or_never().is_none());
        ready.send(()).unwrap();
        let mut values = Vec::new();
        while let Some(resource) = receiver.recv().await {
            let resource = resource.unwrap();
            assert!(Arc::ptr_eq(&resource.retained, &received_retained));
            values.push((*resource.first.0, *resource.second.0, *resource.third.0));
        }
        values
    });
    started.await.unwrap();
    let sender_worker = Worker::new();
    let (mut inputs, mut incoming) = sender_worker
        .channel::<relocade_workers::Sender<Object<Local>>>()
        .await
        .unwrap();
    let send = sender_worker.run(move || async move {
        let mut sender = incoming.recv().await.unwrap().unwrap();
        for (first, second, third) in [(42, 1, 5), (73, 2, 9)] {
            sender
                .send(Object {
                    retained: retained.clone(),
                    first: Local(Rc::new(first)),
                    second: Local(Rc::new(second)),
                    third: Local(Rc::new(third)),
                })
                .unwrap();
        }
    });
    inputs.send(sender).unwrap();
    assert_eq!(receive.await.unwrap(), [(42, 1, 5), (73, 2, 9)]);
    send.await.unwrap();

    let (mut sender, receiver) = channel::<Local>().unwrap();
    drop(receiver);
    assert!(sender.send(Local(Rc::new(99))).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resources_arrive_after_tasks_start_on_existing_workers() {
    pollster::block_on(repeated_delivery());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn resources_arrive_after_tasks_start_on_existing_workers() {
    repeated_delivery().await;
}
