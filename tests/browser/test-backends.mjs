import { cargo } from "./build.mjs";

export function testBackends(filters = []) {
    cargo("test", filters.length ? filters : ["--tests"]);
}

if (import.meta.main) testBackends(process.argv.slice(2));
