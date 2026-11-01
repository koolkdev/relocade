# Frontend integration

Shared contracts connect execution and graphics adapters to the application.
Input uses winit value types; graphics uses the application's wgpu resources.
The application owns its windows, workers and presentation. Each adapter owns
translation into the guest's behavior. The crate creates no workers or windows.

The `keyboard` feature provides owned `KeyboardEvent` messages and ordinary Rust
channel endpoints. Events preserve physical and logical keys, location, text,
press/release state, repeat and modifiers using winit's types. Focus loss releases
transient input state. A BIOS adapter consumes these events on execution and
chooses its supported scan codes and text encoding; other environments can use
the same events with their own input semantics.

The `display` feature provides `display::DisplayRenderer`, the graphics-side
implementation of a display connection. `service(queue)` handles backend activity
independently of redraws, requesting a redraw with `Some(true)`, reporting other
work with `Some(false)`, or ending service with `None`. A backend submits any GPU
work that must progress without presentation itself; the host does not submit on
each service result. VGA can just wait for changed captures here, while a command
backend can execute guest GPU commands and handle completions.

`encode_image(queue, encoder)` queues uploads and records work for the current
image, or reuses an existing image. This operation never submits. The host records
drawing that image later in the same batch and submits it before another service
or encode call. Recording returns without waiting for GPU completion, and image
storage is reusable. Discarding a
recorded batch does not roll back an implementation's image cache and can leave
stale pixels. Guest execution must progress independently of image requests.
The logical connection survives GPU replacement. The host calls `device_lost()`
to invalidate GPU state and let the backend notify its guest, then supplies a new
device through `attach_device(device, queue)`. Successful rebinding resumes
backend work and image encoding, but does not imply guest recovery: the backend
can keep returning no image while processing a guest reset. VGA can restore a capture
automatically without imposing that policy on other implementations.

An existing `relocade_renderer::Image` can be used directly as static content.
The default lifecycle hooks reject device replacement; an image with only GPU
storage cannot reconstruct its contents.

This interface connects a graphics backend to host presentation. The emulated
display device owns registers, memory and timing. `relocade-renderer` provides the GPU image and
image-drawing tools used by the connection and host.
