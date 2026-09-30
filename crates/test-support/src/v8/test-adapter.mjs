const seen = new WeakSet();

export default function execute(modules, input) {
  if (input.exit) process.exit(1);
  if (input.error) throw new Error(input.error);
  const results = modules.map(module => {
    const reused = seen.has(module);
    seen.add(module);
    const instance = new WebAssembly.Instance(module);
    return { result: instance.exports.run(input.delta), reused };
  });
  return input.list ? results : results[0];
}
