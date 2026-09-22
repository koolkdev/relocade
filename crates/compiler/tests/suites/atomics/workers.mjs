import { Worker, parentPort, workerData } from 'node:worker_threads';

if (workerData?.atomicWorker) {
  const { module, memory, start } = workerData;
  const signal = new Int32Array(start);
  const instance = new WebAssembly.Instance(module, { test: { counter: memory } });
  parentPort.postMessage('ready');
  Atomics.wait(signal, 0, 0);
  const observed = [];
  for (let index = 0; index < 1024; index++) observed.push(instance.exports.run());
  parentPort.postMessage(observed);
}

export default async function execute(module) {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  const start = new SharedArrayBuffer(4);
  const workers = [];
  const ready = [];
  const completed = [];
  for (let index = 0; index < 4; index++) {
    const worker = new Worker(new URL(import.meta.url), {
      workerData: { atomicWorker: true, module, memory, start },
    });
    workers.push(worker);
    ready.push(new Promise((resolve, reject) => {
      worker.once('error', reject);
      worker.once('message', message => {
        if (message !== 'ready') reject(new Error('worker did not enter the start barrier'));
        else resolve();
      });
    }));
    completed.push(new Promise((resolve, reject) => {
      worker.once('error', reject);
      worker.on('message', message => {
        if (Array.isArray(message)) resolve(message);
      });
    }));
  }
  try {
    await Promise.all(ready);
    Atomics.store(new Int32Array(start), 0, 1);
    Atomics.notify(new Int32Array(start), 0);
    const observed = (await Promise.all(completed)).flat();
    if (new Uint32Array(memory.buffer)[0] !== 4096) throw new Error('lost atomic increments');
    return observed;
  } finally {
    await Promise.all(workers.map(worker => worker.terminate()));
  }
}
