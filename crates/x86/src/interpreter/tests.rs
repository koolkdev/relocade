use super::build_interpreter;
use crate::support::config::SEGMENTED16;

mod defaults;
mod physical;

#[test]
fn serial_and_parallel_compilation_match_the_embedded_interpreter() {
    let config = SEGMENTED16;
    let serial = build_interpreter(config.entry, config.memory_backend)
        .unwrap()
        .compile()
        .unwrap();
    let (pool, executor) = relocade_workers::Pool::new(vec![
        relocade_workers::Thread::new(),
        relocade_workers::Thread::new(),
    ])
    .unwrap();
    let ((), parallel) = pollster::block_on(futures_util::future::join(pool.run(), async move {
        build_interpreter(config.entry, config.memory_backend)
            .unwrap()
            .compile_with(&executor)
            .await
            .unwrap()
    }));
    assert!(serial == parallel, "executor choice preserves the module");
    assert!(
        serial == relocade_interpreter::module(config.entry, config.memory_backend).unwrap(),
        "build artifact matches fresh generation"
    );
}
