export * from "./pkg/relocade_compiler_worker_tests.js";
import init from "./pkg/relocade_compiler_worker_tests.js";

// web-thread supplies `module`; wasm-bindgen's async initializer calls that
// argument `module_or_path`. Pass the compiled module and shared memory through.
export default function initialize({ module, memory }) {
    return init({ module_or_path: module, memory });
}
