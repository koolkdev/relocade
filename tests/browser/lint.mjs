import { cargo } from "./build.mjs";

await cargo("clippy", ["--all-targets", "--profile", "test", "--", "-D", "warnings"]);
