use super::{HEIGHT, WIDTH};
use miniz_oxide::deflate::compress_to_vec_zlib;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};

pub(super) fn encode(pages: Vec<Vec<u8>>) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let mut ids = Ref::new(1);
    let catalog = ids.bump();
    let tree = ids.bump();
    let ink = Name(b"Ink");
    let mut page_ids = Vec::with_capacity(pages.len());
    pdf.catalog(catalog).pages(tree);

    for page in pages {
        let page_id = ids.bump();
        let image = ids.bump();
        let content = ids.bump();
        page_ids.push(page_id);
        let mut entry = pdf.page(page_id);
        entry
            .parent(tree)
            .media_box(Rect::new(0., 0., 612., 792.))
            .contents(content);
        entry.resources().x_objects().pair(ink, image);
        entry.finish();

        let compressed = compress_to_vec_zlib(&page, 6);
        let mut bitmap = pdf.image_xobject(image, &compressed);
        bitmap
            .width(WIDTH as i32)
            .height(HEIGHT as i32)
            .bits_per_component(1);
        bitmap.color_space().device_gray();
        bitmap.decode([1., 0.]); // A set bit in the DOS glyph represents black ink.
        bitmap.filter(Filter::FlateDecode);
        bitmap.finish();

        let mut drawing = Content::new();
        // PDF units are 1/72 inch. Leave a half-inch margin on each side.
        drawing
            .transform([540., 0., 0., 720., 36., 36.])
            .x_object(ink);
        pdf.stream(content, &drawing.finish());
    }
    pdf.pages(tree).count(page_ids.len() as i32).kids(page_ids);
    pdf.finish()
}
