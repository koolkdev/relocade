use super::{storage::Storage, ArtifactCache};
use std::{cell::Cell, path::Path};

const ARTIFACT: &[u8] = b"generated artifact";
const KEY: &str = "consumer/version/inputs";

fn at(path: &Path) -> ArtifactCache {
    ArtifactCache {
        storage: Some(Storage {
            directory: path.to_owned(),
        }),
    }
}

#[test]
fn artifacts_survive_reopening_and_distinct_keys_do_not_alias() {
    pollster::block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let first = at(directory.path())
            .load_or_generate(KEY, || async { Ok::<_, ()>(ARTIFACT.to_vec()) })
            .await
            .unwrap();
        assert_eq!(first, ARTIFACT);
        let reopened = at(directory.path());
        assert_eq!(
            reopened
                .load_or_generate(KEY, || async {
                    Err::<Vec<u8>, _>("cache hit must skip generation")
                })
                .await
                .unwrap(),
            ARTIFACT
        );
        for other in [
            "other-consumer/version/inputs",
            "consumer/other-version/inputs",
            "consumer/version/other-inputs",
        ] {
            assert_eq!(
                reopened
                    .load_or_generate(other, || async {
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
        let integrity = cacache::write_sync(directory.path(), KEY, ARTIFACT).unwrap();
        cacache::remove_hash_sync(directory.path(), &integrity).unwrap();
        let generated = Cell::new(0);
        let bytes = cache
            .load_or_generate(KEY, || async {
                generated.set(generated.get() + 1);
                Ok::<_, ()>(ARTIFACT.to_vec())
            })
            .await
            .unwrap();
        assert_eq!(bytes, ARTIFACT);
        assert_eq!(generated.get(), 1);
        assert_eq!(cacache::read_sync(directory.path(), KEY).unwrap(), ARTIFACT);

        let blocked = directory.path().join("file");
        std::fs::write(&blocked, b"not a directory").unwrap();
        assert_eq!(
            at(&blocked)
                .load_or_generate(KEY, || async { Ok::<_, ()>(ARTIFACT.to_vec()) })
                .await
                .unwrap(),
            ARTIFACT
        );
    });
}

#[test]
fn generation_errors_are_preserved_and_allow_retry() {
    pollster::block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let cache = at(directory.path());
        assert_eq!(
            cache
                .load_or_generate(KEY, || async { Err::<Vec<u8>, _>("generation failed") })
                .await,
            Err("generation failed")
        );
        assert!(cacache::metadata_sync(directory.path(), KEY)
            .unwrap()
            .is_none());
        assert_eq!(
            cache
                .load_or_generate(KEY, || async { Ok::<_, ()>(ARTIFACT.to_vec()) })
                .await
                .unwrap(),
            ARTIFACT
        );
        let unavailable = ArtifactCache { storage: None };
        assert_eq!(
            unavailable
                .load_or_generate(KEY, || async { Err::<Vec<u8>, _>("generation failed") })
                .await,
            Err("generation failed")
        );
    });
}
