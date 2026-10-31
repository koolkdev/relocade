import init, { run } from "relocade:wasm-bindings";

try {
    await init();
    run();
} catch (error) {
    console.error(error);
}
