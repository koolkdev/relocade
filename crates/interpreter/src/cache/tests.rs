use super::{storage::Storage, Cache};
use crate::{key, ExecutionProfile, SegmentProfile};
use std::{cell::Cell, path::Path};

const WASM: &[u8] = b"\0asm\x01\0\0\0";

fn at(path: &Path) -> Cache {
    Cache {
        storage: Some(Storage {
            directory: path.to_owned(),
        }),
    }
}

#[test]
fn artifacts_survive_reopening_and_keys_separate_profiles_and_revisions() {
    pollster::block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let profile = key(SegmentProfile::Flat32.into());
        let first = at(directory.path())
            .load_or_generate(profile, || async { Ok::<_, ()>(WASM.to_vec()) })
            .await
            .unwrap();
        assert_eq!(first, WASM);
        let reopened = at(directory.path());
        assert_eq!(
            reopened
                .load_or_generate(profile, || async {
                    Err::<Vec<u8>, _>("cache hit must skip generation")
                })
                .await
                .unwrap(),
            WASM
        );
        for other in [
            key(ExecutionProfile::Real16).to_owned(),
            format!("{profile}/new-revision"),
        ] {
            assert_eq!(
                reopened
                    .load_or_generate(&other, || async {
                        Err::<Vec<u8>, _>("different artifact must generate")
                    })
                    .await,
                Err("different artifact must generate")
            );
        }
    });
}

#[test]
fn missing_content_regenerates_and_unwritable_cache_does_not_fail_generation() {
    pollster::block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let cache = at(directory.path());
        let artifact = key(ExecutionProfile::Real16);
        let integrity = cacache::write_sync(directory.path(), artifact, WASM).unwrap();
        cacache::remove_hash_sync(directory.path(), &integrity).unwrap();
        let generated = Cell::new(0);
        let bytes = cache
            .load_or_generate(artifact, || async {
                generated.set(generated.get() + 1);
                Ok::<_, ()>(WASM.to_vec())
            })
            .await
            .unwrap();
        assert_eq!(bytes, WASM);
        assert_eq!(generated.get(), 1);
        assert_eq!(
            cacache::read_sync(directory.path(), artifact).unwrap(),
            WASM
        );

        let blocked = directory.path().join("file");
        std::fs::write(&blocked, b"not a directory").unwrap();
        assert_eq!(
            at(&blocked)
                .load_or_generate(artifact, || async { Ok::<_, ()>(WASM.to_vec()) })
                .await
                .unwrap(),
            WASM
        );
    });
}

#[test]
fn generation_errors_are_preserved_and_allow_retry() {
    pollster::block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let cache = at(directory.path());
        let artifact = key(ExecutionProfile::Real16);
        assert_eq!(
            cache
                .load_or_generate(artifact, || async {
                    Err::<Vec<u8>, _>("generation failed")
                })
                .await,
            Err("generation failed")
        );
        assert!(cacache::metadata_sync(directory.path(), artifact)
            .unwrap()
            .is_none());
        assert_eq!(
            cache
                .load_or_generate(artifact, || async { Ok::<_, ()>(WASM.to_vec()) })
                .await
                .unwrap(),
            WASM
        );
        let unavailable = Cache { storage: None };
        assert_eq!(
            unavailable
                .load_or_generate(artifact, || async {
                    Err::<Vec<u8>, _>("generation failed")
                })
                .await,
            Err("generation failed")
        );
    });
}
