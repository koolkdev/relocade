import init, { start } from "./pkg/relocade_dos_app.js";

await init();
const application = start();
await application.finished.finally(() => application.free());
