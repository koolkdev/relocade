// Observe the production node without changing its audible connection.
export function observeOutput() {
    const NativeNode = globalThis.AudioWorkletNode;
    const observer = new Observer();
    nextContext(context => { observer.context = context; });
    globalThis.AudioWorkletNode = class extends NativeNode {
        constructor(...args) {
            super(...args);
            if (args[0] !== observer.context) return;
            globalThis.AudioWorkletNode = NativeNode;
            observer.node = this;
            observer.analyser = args[0].createAnalyser();
            observer.analyser.fftSize = 4096;
            observer.silence = args[0].createGain();
            observer.silence.gain.value = 0;
            this.connect(observer.analyser);
            observer.analyser.connect(observer.silence);
            observer.silence.connect(args[0].destination);
        }
    };
    return observer;
}

class Observer {
    get ready() { return this.node !== undefined; }

    async check(sampleRate, frequency, bufferedFrames) {
        const deadline = performance.now() + 5000;
        let started, nextWindow, measured;
        while (performance.now() < deadline) {
            if (this.ready && this.context.state === "running") {
                started ??= this.context.currentTime;
                // Flush queued PCM and collect a full new analyser window.
                // Wall time alone cannot establish progress while suspended.
                nextWindow ??= started + (bufferedFrames + this.analyser.fftSize) / sampleRate;
                if (this.context.currentTime >= nextWindow) {
                    measured = this.measure(sampleRate);
                    const matches = frequency === 0
                        ? measured.rms <= 0.0001
                        : measured.rms >= 0.12 && measured.rms <= 0.16
                            && Math.abs(measured.frequency - frequency) <= 20;
                    if (matches) {
                        console.log(`audio ${sampleRate} Hz: tone=${measured.frequency.toFixed(1)} Hz RMS=${measured.rms.toFixed(4)}`);
                        return;
                    }
                    nextWindow = this.context.currentTime + this.analyser.fftSize / sampleRate;
                }
            }
            await new Promise(resolve => setTimeout(resolve, 20));
        }
        const advanced = started === undefined ? 0 : this.context.currentTime - started;
        const output = measured ? `${measured.frequency} Hz, RMS ${measured.rms}` : "no fresh samples";
        throw new Error(`Audio check timed out: expected ${frequency} Hz; observed ${output}; `
            + `context=${this.context?.state ?? "missing"}, node=${this.ready}, audio advanced=${advanced}s`);
    }

    measure(sampleRate) {
        const data = new Float32Array(4096);
        this.analyser.getFloatTimeDomainData(data);
        let energy = 0, crossings = 0;
        for (let i = 0; i < data.length; i++) {
            energy += data[i] * data[i];
            if (i && data[i - 1] <= 0 && data[i] > 0) crossings++;
        }
        const rms = Math.sqrt(energy / data.length);
        const measured = crossings * sampleRate / data.length;
        return { frequency: measured, rms };
    }

    finish() {
        this.analyser?.disconnect();
        this.silence?.disconnect();
    }
}

// Fail before processor ownership is transferred into the worklet.
export function failNextModule() {
    nextContext(context => {
        context.audioWorklet.addModule = () => Promise.reject("test module failure");
    });
}

// wasm-bindgen caches the AudioContext constructor. CPAL reads destination
// synchronously after construction, before any earlier stream can resume setup.
function nextContext(observe) {
    const prototype = BaseAudioContext.prototype;
    const destination = Object.getOwnPropertyDescriptor(prototype, "destination");
    Object.defineProperty(prototype, "destination", {
        ...destination,
        get() {
            Object.defineProperty(prototype, "destination", destination);
            observe(this);
            return destination.get.call(this);
        },
    });
}
