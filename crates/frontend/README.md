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
implementation of a display connection. `update(queue)` waits for source updates
and synchronizes inputs. `encode_image(encoder)` records GPU work for the current
image, or reuses an existing image. The host records drawing that image later in
the same batch and submits it before another update or encode. Recording returns
without waiting for GPU completion, and image storage is reusable. Discarding a
recorded batch does not roll back an implementation's image cache and can leave
stale pixels. Guest execution must progress independently of image requests.
The logical connection survives GPU replacement. The host calls `device_lost()`
to invalidate GPU state and let the backend notify its guest, then supplies a new
device through `attach_device(device, queue)`. Successful rebinding resumes
updates and image encoding, but does not imply guest recovery: the backend can keep
returning no image while processing a guest reset. VGA can restore a capture
automatically without imposing that policy on other implementations.

An existing `relocade_renderer::Image` can be used directly as static content.
The default lifecycle hooks reject device replacement; an image with only GPU
storage cannot reconstruct its contents.

This interface describes a consumer of display state. The emulated display device
owns registers, memory and timing. `relocade-renderer` provides the GPU image and
image-drawing tools used by the connection and host.
