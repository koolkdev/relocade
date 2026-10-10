// A generator instance belongs exclusively to its compilation worker.
export class Generator {
  constructor(instance) { this.exports = instance.exports; }

  generate(request) {
    const input = new TextEncoder().encode(JSON.stringify(request));
    const api = this.exports;
    const pointer = api.request_buffer(input.length) >>> 0;
    if (pointer === 0) throw new Error('compilation request is too large');
    new Uint8Array(api.memory.buffer, pointer, input.length).set(input);
    api.generate();
    // Generation can grow memory; obtain fresh views after the call.
    const metadata = JSON.parse(new TextDecoder().decode(new Uint8Array(
      api.memory.buffer, api.metadata_pointer() >>> 0, api.metadata_length(),
    )));
    if (metadata.error !== undefined) throw new Error(metadata.error);
    const bytes = new Uint8Array(api.memory.buffer, api.wasm_pointer() >>> 0, api.wasm_length()).slice();
    return { bytes, entry: metadata.entry };
  }
}
