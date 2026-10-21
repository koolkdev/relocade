export * from "./pkg/relocade_threaded_runtime.js";
import init from "./pkg/relocade_threaded_runtime.js";

// web-thread supplies `module`; wasm-bindgen's async initializer calls that
// argument `module_or_path`. Pass the compiled module and shared memory through.
export default function initialize({ module, memory }) {
    return init({ module_or_path: module, memory });
}
