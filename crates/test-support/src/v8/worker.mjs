import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const modules = new Map();
for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
  let response;
  try {
    const request = JSON.parse(line);
    const compiled = request.modules.map(({ id, wasm }) => {
      if (wasm !== null) modules.set(id, new WebAssembly.Module(readFileSync(wasm)));
      if (!modules.has(id)) throw new Error(`unknown module ${id}`);
      return modules.get(id);
    });
    const { default: execute } = await import(pathToFileURL(request.adapter).href);
    response = { status: 'ok', value: await execute(compiled, request.input) };
  } catch (error) {
    response = { status: 'error', value: error.stack ?? String(error) };
  }
  process.stdout.write(`${JSON.stringify(response)}\n`);
}
