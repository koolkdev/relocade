import { parentPort, workerData } from 'node:worker_threads';
import { readFile } from 'node:fs/promises';
import { Generator } from './generator.mjs';

// Register the port before awaiting compilation: a pending Wasm compilation
// promise alone does not keep a Node worker's event loop alive.
const generatorReady = readFile(new URL(workerData.generator))
  .then(bytes => WebAssembly.compile(bytes))
  .then(module => WebAssembly.instantiate(module))
  .then(instance => ({ generator: new Generator(instance) }), error => ({ error }));

// Serial requests keep generation's private buffers and the pending-work bound
// simple. Engine modules can be cloned to the execution thread without a store.
let queue = Promise.resolve();
parentPort.on('message', ({ id, request }) => {
  queue = queue.then(async () => {
    try {
      const { generator, error } = await generatorReady;
      if (error) throw error;
      const generated = generator.generate(request);
      const module = await WebAssembly.compile(generated.bytes);
      parentPort.postMessage({ id, module, entry: generated.entry });
    } catch (error) {
      parentPort.postMessage({ id, error: String(error) });
    }
  });
});
