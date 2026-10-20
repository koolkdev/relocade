// A port message schedules a task, not another Promise microtask. A persistent
// channel also avoids the nested-timer clamp on a busy CPU worker.
let channel;
const waiting = [];
export function nextTask() {
    if (!channel) {
        channel = new MessageChannel();
        channel.port1.onmessage = () => waiting.shift()();
    }
    return new Promise(resolve => {
        waiting.push(resolve);
        channel.port2.postMessage(0);
    });
}
