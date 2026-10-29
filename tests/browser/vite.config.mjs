import { fileURLToPath } from "node:url";
import { webConfig } from "../../tools/web/vite.mjs";

export default webConfig(
    fileURLToPath(new URL(".", import.meta.url)),
    fileURLToPath(new URL("./pkg/relocade_browser_tests.js", import.meta.url)),
);
