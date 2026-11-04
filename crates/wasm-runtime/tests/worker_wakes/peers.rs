use super::*;

async fn peers() {
    for close_host in [false, true] {
        let (released, release) = oneshot::channel();
        let signal = Signal::new(released);
        let wakes = signal.wakes.clone();
        let (host, endpoint) = wake_connection(signal.clone());
        let peer = host.peer();
        let (send_waker, receive_waker) = oneshot::channel();
        let (finish, finished) = oneshot::channel();
        let owner = Thread::new();
        let task = owner.run_resource(signal, move |signal| async move {
            let connection = endpoint.bind(signal);
            send_waker.send(connection.waker().clone()).unwrap();
            finished.await.unwrap();
        });
        let waker = receive_waker.await.unwrap();
        let (ready, bound) = oneshot::channel();
        let (check_closed, closed) = oneshot::channel();
        let thread = Thread::new();
        let peer_task = thread.run_resource(peer, move |peer| async move {
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
