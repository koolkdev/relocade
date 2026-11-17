use super::{Access, Directory};
use std::io::{self, ErrorKind};

pub async fn write_contract(root: Directory, read_only: Directory) -> io::Result<()> {
    let directory = root.create_directory("SAVES").await?;
    let file = directory.create_file("STATE.DAT").await?;
    file.write(b"abcdef").await?;
    assert_eq!(file.write_at(b"XY", 2).await?, 2);
    assert_eq!(file.read().await?, b"abXYef");
    file.set_len(2).await?;
    file.set_len(5).await?;
    file.write_at(b"Z", 7).await?;
    assert_eq!(file.read().await?, b"ab\0\0\0\0\0Z");
    assert_eq!(file.write_at(b"", 100).await?, 0);
    assert_eq!(file.metadata().await?.len, 8);
    file.write(b"ok").await?;
    assert_eq!(file.len().await?, 2);
    // Opening an existing file/directory through create preserves its contents.
    assert_eq!(
        root.create_directory("SAVES")
            .await?
            .create_file("STATE.DAT")
            .await?
            .read()
            .await?,
        b"ok"
    );
    let readonly_file = read_only
        .open_directory("SAVES")
        .await?
        .open_file("STATE.DAT", Access::ReadOnly)
        .await?;
    assert_eq!(
        readonly_file.write(b"denied").await.unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        readonly_file.set_len(0).await.unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        read_only.create_file("DENIED").await.unwrap_err().kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        read_only
            .create_directory("DENIED")
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::PermissionDenied
    );
    for name in ["..", "../outside", "/absolute", "a/b", "a\\b", "C:drive"] {
        assert_eq!(
            root.create_file(name).await.unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(readonly_file.read().await?, b"ok");
    file.write(b"").await?;
    assert!(file.is_empty().await?);
    file.write(b"ok").await?;
    assert!(file.metadata().await?.modified.is_some());
    Ok(())
}
