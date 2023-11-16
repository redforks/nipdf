use anyhow::Result as AnyResult;
use criterion::{criterion_group, criterion_main, Criterion};
use nipdf::file::{File, RenderOptionBuilder};

fn read_sample_file(file_path: impl AsRef<std::path::Path>) -> Vec<u8> {
    use std::path::Path;

    let file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("sample_files")
        .join(file_path);
    std::fs::read(file_path).unwrap()
}

/// Render specific page of pdf file.
/// `file_path` relative to '~/sample_files/'.
fn render_page(file_path: impl AsRef<std::path::Path>, no: usize) -> AnyResult<tiny_skia::Pixmap> {
    let buf = read_sample_file(file_path);
    let f = File::parse(buf, "", "")?;
    let resolver = f.resolver()?;
    let pages = f.catalog(&resolver)?.pages()?;
    let page = &pages[no];
    let option = RenderOptionBuilder::new().zoom(1.5);
    Ok(page.render(option)?)
}

pub fn render1(c: &mut Criterion) {
    c.bench_function("page render", |b| {
        b.iter(|| render_page("../../../../ICEpower125ASX2_Datasheet_2.0.pdf", 1).unwrap())
    });
}

pub fn render2(c: &mut Criterion) {
    c.bench_function("page render", |b| {
        b.iter(|| render_page("../../../../compressed.tracemonkey-pldi-09.pdf", 0).unwrap())
    });
}

criterion_group! {
  name = benches;
  config = Criterion::default();
  targets = render1, render2
}
criterion_main!(benches);
