import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import { webConfig } from "../../tools/web/vite.mjs";

export default defineConfig(webConfig(
    fileURLToPath(new URL(".", import.meta.url)),
    fileURLToPath(new URL("./pkg/relocade_browser_tests.js", import.meta.url)),
));
