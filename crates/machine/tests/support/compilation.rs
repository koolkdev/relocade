use relocade_machine::worker::{Pool, PoolHandle, Thread};

/// Keep a fixed compilation pool running through the fixture's machine shutdowns.
pub async fn run<T>(test: impl AsyncFnOnce(PoolHandle) -> T) -> T {
    let (pool, jobs) = Pool::new(vec![Thread::new()]).unwrap();
    let ((), result) = futures_util::future::join(pool.run(), test(jobs)).await;
    result
}
