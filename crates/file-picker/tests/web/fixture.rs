use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
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
    #[wasm_bindgen(js_name = completeFilePicker)]
    pub fn complete_file_picker(cancel: bool, modified: f64);
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
