use super::*;
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::sync::atomic::AtomicBool;

async fn peers() {
    for close_host in [false, true] {
        let (released, release) = oneshot::channel();
        let signal = Signal::new(released);
        let wakes = signal.wakes.clone();
        let (host, endpoint) = wake_connection(signal.clone());
        let peer = host.peer();
        let (send_waker, receive_waker) = oneshot::channel();
        let (finish, finished) = oneshot::channel();
        let owner = Worker::new();
        let (mut resources, mut resource_input) = owner.channel().await.unwrap();
        resources.send(signal).unwrap();
        let task = owner.run(move || async move {
            let signal = resource_input.recv().await.unwrap().unwrap();
            let connection = endpoint.bind(signal);
            send_waker.send(connection.waker().clone()).unwrap();
            finished.await.unwrap();
        });
        let waker = receive_waker.await.unwrap();
        let (ready, bound) = oneshot::channel();
        let (check_closed, closed) = oneshot::channel();
        let thread = Worker::new();
        let (mut resources, mut resource_input) = thread.channel().await.unwrap();
        resources.send(peer).unwrap();
        let peer_task = thread.run(move || async move {
            let peer = resource_input.recv().await.unwrap().unwrap();
            let first = peer.clone().bind();
            let second = peer.clone().bind();
            waker.wake_by_ref();
            assert_eq!(wakes.load(Ordering::SeqCst), 1, "peer wakes synchronously");
            drop(first);
            waker.wake_by_ref();
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                2,
                "duplicate guard remains bound"
            );
            drop(second);
            let _replacement = peer.bind();
            waker.wake_by_ref();
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                3,
                "peer removal does not close the connection"
            );
            ready.send(()).unwrap();
            closed.await.unwrap();
            waker.wake_by_ref();
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                3,
                "closed owners disable escaped peers"
            );
        });
        bound.await.unwrap();
        if close_host {
            drop(host);
            check_closed.send(()).unwrap();
            peer_task.await.unwrap();
            finish.send(()).unwrap();
            task.await.unwrap();
        } else {
            finish.send(()).unwrap();
            task.await.unwrap();
            check_closed.send(()).unwrap();
            peer_task.await.unwrap();
            drop(host);
        }
        release.await.unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn posted_peers_share_bindings_and_follow_the_original_owners_lifetime() {
    pollster::block_on(peers());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn posted_peers_share_bindings_and_follow_the_original_owners_lifetime() {
    peers().await;
}

fn spin_until(mut ready: impl FnMut() -> bool) {
    for _ in 0..50_000_000 {
        if ready() {
            return;
        }
        std::hint::spin_loop();
    }
    panic!("notification did not reach its owner");
}

async fn unbound_callbacks() {
    for close_host in [false, true] {
        let (released, release) = oneshot::channel();
        let signal = Signal::new(released);
        let wakes = signal.wakes.clone();
        let (host, endpoint) = wake_connection(signal.clone());
        let connection = endpoint.bind(signal.clone());
        let waker = connection.waker().clone();
        let (commands, mut requests) = mpsc::unbounded::<(bool, oneshot::Sender<()>)>();
        let (ready, started) = oneshot::channel();
        let published = Arc::new(AtomicBool::new(false));
        let callback_published = published.clone();
        let thread = Worker::new();
        let (mut resources, mut resource_input) = thread.channel().await.unwrap();
        resources.send(signal).unwrap();
        let task = thread.run(move || async move {
            let signal = resource_input.recv().await.unwrap().unwrap();
            // No WakePeer binding: a foreign callback only has the Rust waker.
            // The signal resource is retained solely to observe relay delivery.
            ready.send(()).unwrap();
            while let Some((observe, done)) = requests.next().await {
                for _ in 0..100 {
                    waker.wake_by_ref();
                }
                callback_published.store(true, Ordering::SeqCst);
                if observe {
                    spin_until(|| signal.memory.atomic_load::<u8>(0) != 0);
                }
                done.send(()).unwrap();
            }
        });
        started.await.unwrap();
        let (done, observed) = oneshot::channel();
        commands.unbounded_send((true, done)).unwrap();
        // Keep the browser host out of its event loop until the whole burst is
        // queued, so forwarding must coalesce it into one request.
        spin_until(|| published.load(Ordering::SeqCst));
        observed.await.unwrap();
        #[cfg(target_arch = "wasm32")]
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(wakes.load(Ordering::SeqCst), 100);

        published.store(false, Ordering::SeqCst);
        let (done, queued) = oneshot::channel();
        commands.unbounded_send((false, done)).unwrap();
        spin_until(|| published.load(Ordering::SeqCst));
        let before_close = wakes.load(Ordering::SeqCst);
        let mut host = Some(host);
        let mut connection = Some(connection);
        if close_host {
            drop(host.take());
        } else {
            drop(connection.take());
        }
        queued.await.unwrap();
        let (done, stale) = oneshot::channel();
        commands.unbounded_send((false, done)).unwrap();
        stale.await.unwrap();
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            before_close,
            "queued and stale wakes stay disabled"
        );
        drop(commands);
        task.await.unwrap();
        drop(connection);
        drop(host);
        release.await.unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unbound_callback_wakes_coalesce_and_end_with_either_owner() {
    pollster::block_on(unbound_callbacks());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unbound_callback_wakes_coalesce_and_end_with_either_owner() {
    unbound_callbacks().await;
}
