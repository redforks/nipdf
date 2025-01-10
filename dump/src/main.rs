#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]

use clap::{Command, arg, value_parser};
use image::ImageFormat;
use mimalloc::MiMalloc;
use nipdf::{
    file::File,
    object::{Object, RuntimeObjectId},
};
use nipdf_render::{RenderOptionBuilder, render_steps};
use snafu::{OptionExt, ResultExt, Whatever, report};
use std::{
    collections::HashSet,
    io::{BufWriter, Cursor, copy, stdout},
    path::{Path, PathBuf},
};
type Result<T, E = Whatever> = std::result::Result<T, E>;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn cli() -> Command {
    Command::new("dump-pdf")
        .about("Dump PDF file structure and contents")
        .subcommand_required(true)
        .subcommand(
            Command::new("stream")
                .about("dump stream content to stdout")
                .arg(
                    arg!(-f <filename> "PDF file to dump")
                        .value_parser(value_parser!(PathBuf))
                        .required(true),
                )
                .arg(arg!(-p --password <password> "Password for encrypted PDF file"))
                .arg(
                    arg!(<object_id> "object ID to dump")
                        .value_parser(value_parser!(u32))
                        .required(true),
                )
                .arg(arg!(--raw "Skip decoding stream content"))
                .arg(arg!(--png "Assume stream is image, decode and convert to PNG")),
        )
        .subcommand(
            Command::new("page")
                .about("dump page content to stdout")
                .arg(
                    arg!(-f <filename> "PDF file to dump")
                        .value_parser(value_parser!(PathBuf))
                        .required(true),
                )
                .arg(arg!(-p --password <password> "Password for encrypted PDF file"))
                .arg(arg!(--pages "display total page numbers"))
                .arg(arg!(--id "display page object ID"))
                .arg(arg!(--png "Render page to PNG"))
                .arg(arg!(--zoom [zoom] "Zoom factor for PNG rendering, default: 1.75"))
                .arg(arg!(--"no-crop" "Do not apply CropBox"))
                .arg(arg!(--steps <steps> "Stop render after <steps> graphic steps"))
                .arg(arg!([page_no] "page number (start from zero) to dump")),
        )
        .subcommand(
            Command::new("object")
                .about("dump pdf object by id")
                .arg(
                    arg!(-f <filename> "PDF file to dump")
                        .value_parser(value_parser!(PathBuf))
                        .required(true),
                )
                .arg(arg!(-p --password <password> "Password for encrypted PDF file"))
                .arg(
                    arg!([object_id] "object ID to dump")
                        .value_parser(value_parser!(u32))
                        .required(true),
                ),
        )
}

fn open(path: impl AsRef<Path>, password: &str) -> Result<File> {
    let buf = std::fs::read(path).whatever_context("read file")?;
    File::parse(buf, password).whatever_context("Open pdf file")
}

fn dump_stream(path: &PathBuf, password: &str, id: u32, raw: bool, as_png: bool) -> Result<()> {
    let f = open(path, password)?;
    let resolver = f.resolver().whatever_context("resolve")?;
    let obj = resolver
        .resolve(id)
        .with_whatever_context(|_| format!("resolve object: {}", id))?;
    match obj {
        Object::Stream(s) => {
            let decoded;
            let png_buffer;
            let mut buf = if raw {
                s.raw(&resolver)
                    .with_whatever_context(|_| format!("decode raw stream : {}", id))?
            } else if as_png {
                let img = s
                    .decode_image(&resolver, None)
                    .with_whatever_context(|_| format!("decode image: {}", id))?;
                let mut buf = Cursor::new(Vec::new());
                img.write_to(&mut buf, ImageFormat::Png)
                    .whatever_context("encode image to png")?;
                png_buffer = buf.into_inner();
                &png_buffer
            } else {
                decoded = s
                    .decode(&resolver)
                    .with_whatever_context(|_| format!("decode stream: {}", id))?;
                decoded.as_ref()
            };
            copy(&mut buf, &mut BufWriter::new(&mut stdout()))
                .whatever_context("write to stdout")?;
        }
        _ => eprintln!("object is not a stream"),
    };
    Ok(())
}

struct DumpPageArgs<'a> {
    path: &'a PathBuf,
    password: &'a str,
    page_no: Option<u32>,
    show_total_pages: bool,
    show_page_id: bool,
    to_png: bool,
    steps: Option<usize>,
    zoom: Option<f32>,
    no_crop: bool,
}

fn dump_page(args: &DumpPageArgs<'_>) -> Result<()> {
    let DumpPageArgs {
        path,
        password,
        page_no,
        show_total_pages,
        show_page_id,
        to_png,
        steps,
        zoom,
        no_crop,
    } = *args;

    let f = open(path, password)?;
    let resolver = f.resolver().whatever_context("get resolver")?;
    let catalog = f.catalog(&resolver).whatever_context("get catalog")?;

    if show_total_pages {
        println!("{}", catalog.pages().whatever_context("get pages")?.len());
    } else if show_page_id {
        let page_no = page_no.whatever_context("page number is required")?;
        let page = &catalog.pages().whatever_context("get pages")?[page_no as usize];
        println!("{}", page.id());
    } else if to_png {
        let page_no = page_no.whatever_context("page number is required")?;
        let page = &catalog.pages().whatever_context("get pages")?[page_no as usize];
        let image = render_steps(
            page,
            RenderOptionBuilder::new().zoom(zoom.unwrap_or(1.75)),
            steps,
            no_crop,
        )
        .whatever_context("render page")?;
        let mut buf = vec![];
        let mut cursor = Cursor::new(&mut buf);
        image
            .write_to(&mut cursor, ImageFormat::Png)
            .whatever_context("encode to png")?;
        copy(&mut &buf[..], &mut BufWriter::new(&mut stdout()))
            .whatever_context("write to stdout")?;
    } else if let Some(page_no) = page_no {
        let page = &catalog.pages().whatever_context("get pages")?[page_no as usize];
        let contents = page.content().whatever_context("get page content")?;
        for op in contents.operations() {
            println!("{:?}", op);
        }
    }

    Ok(())
}

fn dump_object(path: &PathBuf, password: &str, id: u32) -> Result<()> {
    let f = open(path, password)?;
    let resolver = f.resolver().whatever_context("get resolver")?;

    let id = RuntimeObjectId(id);
    let mut id_wait_scanned = vec![id];
    let mut ids = HashSet::new();
    while let Some(id) = id_wait_scanned.pop() {
        if ids.insert(id) {
            println!("OBJ {}:", id);
            let obj = resolver
                .resolve(id)
                .with_whatever_context(|_| format!("resolve id: {}", id))?;
            obj.to_doc()
                .render(80, &mut stdout())
                .whatever_context("render")?;
            print!("\n\n\n");

            id_wait_scanned.extend(obj.iter_values().filter_map(|o| {
                if let Object::Reference(r) = o {
                    Some(r.id().id())
                } else {
                    None
                }
            }));
        }
    }

    Ok(())
}

#[report]
fn main() -> Result<()> {
    env_logger::init();

    match cli().get_matches().subcommand() {
        Some(("stream", sub_m)) => dump_stream(
            sub_m.get_one("filename").unwrap(),
            sub_m
                .get_one::<String>("password")
                .map_or_else(|| "", |p| p.as_str()),
            *sub_m.get_one::<u32>("object_id").unwrap(),
            sub_m.get_one::<bool>("raw").copied().unwrap_or_default(),
            sub_m.get_one::<bool>("png").copied().unwrap_or_default(),
        ),
        Some(("page", sub_m)) => dump_page(&DumpPageArgs {
            path: sub_m.get_one::<PathBuf>("filename").unwrap(),
            password: sub_m
                .get_one::<String>("password")
                .map_or_else(|| "", |p| p.as_str()),
            page_no: sub_m
                .get_one::<String>("page_no")
                .and_then(|s| s.parse().ok()),
            show_total_pages: sub_m.get_one::<bool>("pages").copied().unwrap_or_default(),
            show_page_id: sub_m.get_one::<bool>("id").copied().unwrap_or_default(),
            to_png: sub_m.get_one::<bool>("png").copied().unwrap_or_default(),
            steps: sub_m
                .get_one::<String>("steps")
                .and_then(|s| s.parse().ok()),
            zoom: sub_m.get_one::<String>("zoom").and_then(|s| s.parse().ok()),
            no_crop: sub_m
                .get_one::<bool>("no-crop")
                .copied()
                .unwrap_or_default(),
        }),
        Some(("object", sub_m)) => dump_object(
            sub_m.get_one("filename").unwrap(),
            sub_m
                .get_one::<String>("password")
                .map_or_else(|| "", |p| p.as_str()),
            *sub_m.get_one::<u32>("object_id").unwrap(),
        ),
        _ => todo!(),
    }
}
