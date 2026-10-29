# Application tasks

`Application` supplies launch data and a worker-local `run` future. Its input,
published source and result are application-defined types; no window or GPU is required.
`Context` receives input, uses an embedding-supplied timer handle and publishes
latest-only sources without scheduling rendering.

```rust,ignore
let mut task = relocade_app::spawn(application, clock);
task.send_input(input);
let source = task.current_source();
task.stop();
let result = task.await?;
```

The embedding drives the corresponding `TimerDriver` while execution is alive.
Completion returns `Some(output)` or an error. Cancellation returns `None` after
worker-local state is destroyed. Retain and await the task through shutdown;
dropping it early can terminate a browser worker before Rust cleanup completes.
The last published source remains readable after execution completes.

This uses the existing machine worker and timer infrastructure. It adds no executor
or CPU loop; CPU consumers continue using the machine's `Runner`.
