// A port message schedules a task, rather than another Promise microtask.
// This also avoids the nested-timer clamp on CPU-bound execution slices.
let channel;
const waiting = [];
export function yieldExecution() {
    if (!channel) {
        channel = new MessageChannel();
        channel.port1.onmessage = () => waiting.shift()();
    }
    return new Promise(resolve => {
        waiting.push(resolve);
        channel.port2.postMessage(0);
    });
}
