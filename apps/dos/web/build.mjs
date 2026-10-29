import { fileURLToPath } from "node:url";
import { buildBindings } from "../../../tools/web/build.mjs";
buildBindings("relocade-dos-app", fileURLToPath(new URL("./pkg", import.meta.url)));
