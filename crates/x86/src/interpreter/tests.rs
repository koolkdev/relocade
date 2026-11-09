use super::build_interpreter;
use crate::SegmentProfile;

#[test]
fn serial_and_parallel_compilation_match_the_cached_interpreter() {
    let profile = SegmentProfile::Segmented16;
    let serial = build_interpreter(profile).unwrap().compile().unwrap();
    let (pool, executor) = relocade_workers::Pool::new(vec![
        relocade_workers::Thread::new(),
        relocade_workers::Thread::new(),
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
    let cached = pollster::block_on(relocade_interpreter::load_or_generate(
        profile.into(),
        || std::future::ready(Ok::<_, relocade_compiler::BuildError>(parallel)),
    ))
    .unwrap();
    assert!(serial == cached, "cached artifact matches fresh generation");
}
