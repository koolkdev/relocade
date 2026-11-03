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

    check(sampleRate, frequency) {
        const data = new Float32Array(4096);
        this.analyser.getFloatTimeDomainData(data);
        let energy = 0, crossings = 0;
        for (let i = 0; i < data.length; i++) {
            energy += data[i] * data[i];
            if (i && data[i - 1] <= 0 && data[i] > 0) crossings++;
        }
        const rms = Math.sqrt(energy / data.length);
        const measured = crossings * sampleRate / data.length;
        if (frequency === 0) {
            if (rms > 0.0001) throw new Error(`Expected silence, RMS ${rms}`);
        } else if (rms < 0.12 || rms > 0.16 || Math.abs(measured - frequency) > 20) {
            throw new Error(`Unexpected tone: ${measured} Hz, RMS ${rms}`);
        }
        console.log(`audio ${sampleRate} Hz: tone=${measured.toFixed(1)} Hz RMS=${rms.toFixed(4)}`);
    }

    finish() {
        this.analyser.disconnect();
        this.silence.disconnect();
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
