import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const modules = new Map();
for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
  let response;
  try {
    const request = JSON.parse(line);
    if (request.wasm !== null) {
      modules.set(request.module, new WebAssembly.Module(readFileSync(request.wasm)));
    }
    const module = modules.get(request.module);
    if (!module) throw new Error(`unknown module ${request.module}`);
    const { default: execute } = await import(pathToFileURL(request.adapter).href);
    response = { status: 'ok', value: await execute(module, request.input) };
  } catch (error) {
    response = { status: 'error', value: error.stack ?? String(error) };
  }
  process.stdout.write(`${JSON.stringify(response)}\n`);
}
