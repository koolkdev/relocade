import init, { run } from "relocade:wasm-bindings";

try {
    await init();
    run();
} catch (error) {
    document.querySelector("#error").textContent = String(error);
    console.error(error);
}
