use super::{Access, File};
use futures_lite::future::zip;
use std::{io, time::SystemTime};

pub async fn read_contract(file: File, modified: Option<SystemTime>) -> io::Result<()> {
    fn rust_handle<T: Clone + Send + Sync>() {}
    rust_handle::<File>();
    assert_eq!(file.access(), Access::ReadOnly);
    let metadata = file.metadata().await?;
    assert_eq!(metadata.len, 6);
    assert_eq!(metadata.modified, modified);
    assert_eq!(file.read().await?, b"abcdef");
    let clone = file.clone();
    let mut beginning = [0; 2];
    let mut ending = [0; 2];
    let (a, b) = zip(
        file.read_at(&mut beginning, 0),
        clone.read_at(&mut ending, 4),
    )
    .await;
    assert_eq!((a?, b?), (2, 2));
    assert_eq!(beginning, *b"ab");
    assert_eq!(ending, *b"ef");
    assert_eq!(file.read_at(&mut ending, 100).await?, 0);
    assert_eq!(file.len().await?, 6);
    assert!(!file.is_empty().await?);
    Ok(())
}
