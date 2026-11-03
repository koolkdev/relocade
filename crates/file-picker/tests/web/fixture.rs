use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export async function directoryHandle() {
    const root = await navigator.storage.getDirectory();
    return root.getDirectoryHandle(`relocade-${crypto.randomUUID()}`, { create: true });
}
export async function removeDirectory(directory) {
    const root = await navigator.storage.getDirectory();
    await root.removeEntry(directory.name, { recursive: true });
}
export function mockPicker() {
    const previous = Object.getOwnPropertyDescriptor(window, 'showDirectoryPicker');
    return {
        mode: null,
        receiver: null,
        set(response, handle) {
            const fixture = this;
            const picker = response === 'missing' ? undefined : function(options) {
                fixture.mode = options.mode;
                fixture.receiver = this;
                if (response === 'throw') throw new DOMException('Requires activation', 'SecurityError');
                if (response === 'ok') return Promise.resolve(handle);
                return Promise.reject(new DOMException('Selection refused', response));
            };
            Object.defineProperty(window, 'showDirectoryPicker', { configurable: true, value: picker });
        },
        restore() {
            if (previous) Object.defineProperty(window, 'showDirectoryPicker', previous);
            else delete window.showDirectoryPicker;
        },
    };
}
export async function seedDirectory(directory) {
    const child = await directory.getDirectoryHandle('DATA', { create: true });
    const file = await child.getFileHandle('GAME.EXE', { create: true });
    const stream = await file.createWritable();
    await stream.write('abcdef');
    await stream.close();
    return (await file.getFile()).lastModified;
}
export function completeFilePicker(cancel, modified) {
    // The fixture supplies the selection, without opening another native dialog.
    const click = HTMLInputElement.prototype.click;
    HTMLInputElement.prototype.click = function() {
        if (this.type !== 'file') click.call(this);
    };
    queueMicrotask(() => {
        HTMLInputElement.prototype.click = click;
        const input = document.querySelector('#rfd-input');
        if (!input) throw new Error('File picker did not create an input');
        if (cancel) {
            input.dispatchEvent(new Event('cancel'));
        } else {
            const transfer = new DataTransfer();
            transfer.items.add(new File(['abcdef'], 'PROGRAM.COM', { lastModified: modified }));
            input.files = transfer.files;
            input.dispatchEvent(new Event('change'));
        }
    });
}
export function holdNextWrite() {
    const original = FileSystemFileHandle.prototype.createWritable;
    let entered, release, complete, fail;
    const arrival = new Promise(resolve => entered = resolve);
    const gate = new Promise(resolve => release = resolve);
    const finished = new Promise((resolve, reject) => { complete = resolve; fail = reject; });
    FileSystemFileHandle.prototype.createWritable = async function(options) {
        const stream = await original.call(this, options);
        const handle = this;
        const close = stream.close.bind(stream);
        stream.close = async () => {
            try {
                await close();
                complete(await (await handle.getFile()).text());
            } catch (error) { fail(error); throw error; }
        };
        entered();
        await gate;
        return stream;
    };
    return {
        entered: () => arrival,
        finished: () => finished,
        release() { FileSystemFileHandle.prototype.createWritable = original; release(); },
    };
}
export function holdNextRead() {
    const original = Blob.prototype.arrayBuffer;
    let entered, release, first = true;
    const arrival = new Promise(resolve => entered = resolve);
    const gate = new Promise(resolve => release = resolve);
    Blob.prototype.arrayBuffer = async function() {
        if (first) { first = false; entered(); await gate; }
        return original.call(this);
    };
    return {
        entered: () => arrival,
        release() { Blob.prototype.arrayBuffer = original; release(); },
    };
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = directoryHandle)]
    pub async fn directory_handle() -> JsValue;
    #[wasm_bindgen(js_name = removeDirectory)]
    pub async fn remove_directory(directory: &JsValue);
    pub type PickerFixture;
    #[wasm_bindgen(js_name = mockPicker)]
    pub fn mock_picker() -> PickerFixture;
    #[wasm_bindgen(method)]
    pub fn set(this: &PickerFixture, response: &str, directory: &JsValue);
    #[wasm_bindgen(method, getter)]
    pub fn mode(this: &PickerFixture) -> String;
    #[wasm_bindgen(method, getter)]
    pub fn receiver(this: &PickerFixture) -> JsValue;
    #[wasm_bindgen(method)]
    pub fn restore(this: &PickerFixture);
    #[wasm_bindgen(js_name = seedDirectory)]
    pub async fn seed_directory(directory: &JsValue) -> f64;

    #[wasm_bindgen(js_name = completeFilePicker)]
    pub fn complete_file_picker(cancel: bool, modified: f64);
    pub type WriteBarrier;
    #[wasm_bindgen(js_name = holdNextWrite)]
    pub fn hold_next_write() -> WriteBarrier;
    #[wasm_bindgen(method, js_name = entered)]
    pub async fn write_started(this: &WriteBarrier);
    #[wasm_bindgen(method, js_name = finished)]
    pub async fn write_finished(this: &WriteBarrier) -> String;
    #[wasm_bindgen(method, js_name = release)]
    pub fn release_write(this: &WriteBarrier);
    pub type ReadBarrier;
    #[wasm_bindgen(js_name = holdNextRead)]
    pub fn hold_next_read() -> ReadBarrier;
    #[wasm_bindgen(method)]
    pub async fn entered(this: &ReadBarrier);
    #[wasm_bindgen(method)]
    pub fn release(this: &ReadBarrier);
}

pub struct ReleaseReads(pub ReadBarrier);
impl Drop for ReleaseReads {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub struct RestorePicker(pub PickerFixture);
impl Drop for RestorePicker {
    fn drop(&mut self) {
        self.0.restore();
    }
}

pub struct ReleaseWrite(pub WriteBarrier);
impl Drop for ReleaseWrite {
    fn drop(&mut self) {
        self.0.release_write();
    }
}
