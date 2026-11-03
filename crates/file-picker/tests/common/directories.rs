use super::{Access, DirEntry, Directory, Metadata};
use std::io::{self, ErrorKind};

pub async fn browse_contract(
    root: Directory,
    directory_metadata: Metadata,
    file_metadata: Metadata,
) -> io::Result<()> {
    fn rust_handle<T: Clone + Send + Sync>() {}
    rust_handle::<Directory>();
    assert_eq!(root.access(), Access::ReadOnly);
    assert_eq!(
        root.read_dir().await?,
        vec![DirEntry {
            name: "DATA".into(),
            metadata: directory_metadata,
        }]
    );
    let directory = root.open_directory("DATA").await?;
    assert_eq!(directory.access(), Access::ReadOnly);
    assert_eq!(
        directory.read_dir().await?,
        vec![DirEntry {
            name: "GAME.EXE".into(),
            metadata: file_metadata,
        }]
    );
    assert_eq!(
        directory
            .open_file("GAME.EXE", Access::ReadWrite)
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::PermissionDenied
    );
    for name in ["", ".", "..", "../outside", "a/b", "a\\b", "C:drive"] {
        assert_eq!(
            root.open_file(name, Access::ReadOnly)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        root.open_file("MISSING", Access::ReadOnly)
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    let file = directory.open_file("GAME.EXE", Access::ReadOnly).await?;
    drop(root);
    drop(directory);
    assert_eq!(file.read().await?, b"abcdef");
    assert_eq!(file.metadata().await?, file_metadata);
    Ok(())
}
