use std::{
    collections::HashSet,
    io::{copy, stdout, BufWriter, Cursor},
    num::NonZeroU32,
};

use anyhow::Result as AnyResult;

use clap::{arg, Command};
use image::ImageOutputFormat;
use nipdf::{
    file::{File, ObjectResolver, RenderOptionBuilder, XRefTable},
    object::{FilterDecodedData, Object},
};

fn cli() -> Command {
    Command::new("dump-pdf")
        .about("Dump PDF file structure and contents")
        .subcommand_required(true)
        .subcommand(
            Command::new("stream")
                .about("dump stream content to stdout")
                .arg(arg!(-f <filename> "PDF file to dump"))
                .arg(arg!(<object_id> "object ID to dump"))
                .arg(arg!(--raw "Skip decoding stream content"))
                .arg(arg!(--image "Assume stream is image, convert to JPEG or PNG based on stream type"))
                .arg(arg!(--png "Assume stream is image, decode and convert to PNG"))
                ,
        )
        .subcommand(
            Command::new("page")
            .about("dump page content to stdout")
                .arg(arg!(-f <filename> "PDF file to dump"))
                .arg(arg!(--pages "display total page numbers"))
                .arg(arg!(--id "display page object ID"))
                .arg(arg!(--png "Render page to PNG"))
                .arg(arg!(--zoom [zoom] "Zoom factor for PNG rendering, default: 1.75"))
                .arg(arg!(--steps <steps> "Stop render after <steps> graphic steps"))
                .arg(arg!([page_no] "page number (start from zero) to dump")),
        )
        .subcommand(
            Command::new("object")
            .about("dump pdf object by id")
                .arg(arg!(-f <filename> "PDF file to dump"))
                .arg(arg!([object_id] "object ID to dump")),
        )
}

fn open(path: &str, buf: &mut Vec<u8>) -> AnyResult<(File, XRefTable)> {
    *buf = std::fs::read(path)?;
    File::parse(&buf[..])
}

fn dump_stream(
    path: &str,
    id: NonZeroU32,
    raw: bool,
    as_image: bool,
    as_png: bool,
) -> AnyResult<()> {
    let mut buf: Vec<u8> = vec![];
    let (_f, xref) = open(path, &mut buf)?;
    let resolver = ObjectResolver::new(&buf, &xref);
    let obj = resolver.resolve(id)?;
    let png_buffer;
    match obj {
        Object::Stream(s) => {
            let decoded;
            let mut buf = if raw {
                s.raw(&resolver)?
            } else {
                decoded = s.decode(&resolver, as_image)?;
                if as_png {
                    if let FilterDecodedData::Image(ref img) = decoded {
                        let mut buf = Cursor::new(Vec::new());
                        img.write_to(&mut buf, ImageOutputFormat::Png)?;
                        png_buffer = buf.into_inner();
                        &png_buffer
                    } else {
                        decoded.as_bytes()
                    }
                } else {
                    decoded.as_bytes()
                }
            };
            copy(&mut buf, &mut BufWriter::new(&mut stdout()))?;
        }
        _ => eprintln!("object is not a stream"),
    };
    Ok(())
}

fn dump_page(
    path: &str,
    page_no: Option<u32>,
    show_total_pages: bool,
    show_page_id: bool,
    to_png: bool,
    steps: Option<usize>,
    zoom: Option<f32>,
) -> AnyResult<()> {
    let mut buf: Vec<u8> = vec![];
    let (f, xref) = open(path, &mut buf)?;
    let resolver = ObjectResolver::new(&buf, &xref);
    let catalog = f.catalog(&resolver)?;

    if show_total_pages {
        println!("{}", catalog.pages()?.len());
    } else if show_page_id {
        let page_no = page_no.expect("page number is required");
        let page = &catalog.pages()?[page_no as usize];
        println!("{}", page.id());
    } else if to_png {
        let page_no = page_no.expect("page number is required");
        let page = &catalog.pages()?[page_no as usize];
        let pixmap =
            page.render_steps(RenderOptionBuilder::new().zoom(zoom.unwrap_or(1.75)), steps)?;
        let buf = pixmap.encode_png()?;
        copy(&mut &buf[..], &mut BufWriter::new(&mut stdout()))?;
    } else if let Some(page_no) = page_no {
        let page = &catalog.pages()?[page_no as usize];
        let contents = page.content()?;
        for op in contents.operations() {
            println!("{:?}", op);
        }
    }

    Ok(())
}

fn dump_object(path: &str, id: NonZeroU32) -> AnyResult<()> {
    let mut buf: Vec<u8> = vec![];
    let (_f, xref) = open(path, &mut buf)?;
    let resolver = ObjectResolver::new(&buf, &xref);

    let mut id_wait_scaned = vec![id];
    let mut ids = HashSet::new();
    while let Some(id) = id_wait_scaned.pop() {
        if ids.insert(id) {
            println!("OBJ {}:", id);
            let obj = resolver.resolve(id)?;
            obj.to_doc().render(80, &mut stdout())?;
            print!("\n\n\n");

            id_wait_scaned.extend(obj.iter_values().filter_map(|o| {
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

fn main() -> AnyResult<()> {
    env_logger::init();

    match cli().get_matches().subcommand() {
        Some(("stream", sub_m)) => dump_stream(
            sub_m.get_one::<String>("filename").unwrap(),
            sub_m
                .get_one::<String>("object_id")
                .and_then(|s| s.parse().ok())
                .unwrap(),
            sub_m.get_one::<bool>("raw").copied().unwrap_or_default(),
            sub_m.get_one::<bool>("image").copied().unwrap_or_default(),
            sub_m.get_one::<bool>("png").copied().unwrap_or_default(),
        ),
        Some(("page", sub_m)) => dump_page(
            sub_m.get_one::<String>("filename").unwrap(),
            sub_m
                .get_one::<String>("page_no")
                .and_then(|s| s.parse().ok()),
            sub_m.get_one::<bool>("pages").copied().unwrap_or_default(),
            sub_m.get_one::<bool>("id").copied().unwrap_or_default(),
            sub_m.get_one::<bool>("png").copied().unwrap_or_default(),
            sub_m
                .get_one::<String>("steps")
                .and_then(|s| s.parse().ok()),
            sub_m.get_one::<String>("zoom").and_then(|s| s.parse().ok()),
        ),
        Some(("object", sub_m)) => dump_object(
            sub_m.get_one::<String>("filename").unwrap(),
            sub_m
                .get_one::<String>("object_id")
                .and_then(|s| s.parse().ok())
                .unwrap(),
        ),
        _ => todo!(),
    }
}
