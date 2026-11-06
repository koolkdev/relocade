//! Pace scripted input through the finite BIOS ring while the consumer runs.

use std::{
    future::{poll_fn, Future},
    io::{ErrorKind, Write},
    pin::pin,
    task::Poll,
};

pub async fn feed_input<R>(
    mut input: impl Write,
    mut bytes: &[u8],
    run: impl Future<Output = R>,
) -> R {
    let mut run = pin!(run);
    poll_fn(|cx| loop {
        if let Poll::Ready(result) = run.as_mut().poll(cx) {
            assert!(
                bytes.is_empty(),
                "consumer finished before accepting the script"
            );
            return Poll::Ready(result);
        }
        if bytes.is_empty() {
            return Poll::Pending;
        }
        match input.write(bytes) {
            Ok(0) => panic!("nonempty console input made no progress"),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Poll::Pending,
            Err(error) => panic!("console input failed: {error}"),
        }
    })
    .await
}
