use super::build_interpreter;
use crate::SegmentProfile;

#[test]
fn serial_and_parallel_compilation_match_the_embedded_interpreter() {
    let profile = SegmentProfile::Segmented16;
    let serial = build_interpreter(profile).unwrap().compile().unwrap();
    let (pool, executor) = relocade_workers::Pool::new(vec![
        relocade_workers::Worker::new(),
        relocade_workers::Worker::new(),
    ])
    .unwrap();
    let ((), parallel) = pollster::block_on(futures_util::future::join(pool.run(), async move {
        build_interpreter(profile)
            .unwrap()
            .compile_with(&executor)
            .await
            .unwrap()
    }));
    assert!(serial == parallel, "executor choice preserves the module");
    assert!(
        serial == relocade_interpreter::module(profile.into()),
        "build artifact matches fresh generation"
    );
}
