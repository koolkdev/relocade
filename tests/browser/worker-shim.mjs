export * from "relocade:wasm-bindings";
import init from "relocade:wasm-bindings";

// web-thread supplies `module`; wasm-bindgen's async initializer calls that
// argument `module_or_path`. Pass the compiled module and shared memory through.
export default function initialize({ module, memory }) {
    return init({ module_or_path: module, memory });
}
