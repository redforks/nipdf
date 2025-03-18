use clap::{Parser, Subcommand, arg};
use image::ImageFormat;
use mimalloc::MiMalloc;
use nipdf::{
    file::File,
    graphics::parse_operations,
    object::{Object, RuntimeObjectId},
};
use nipdf_render::{RenderOptionBuilder, render_steps};
use prescript::Result;
use snafu::{OptionExt as _, ResultExt as _, report};
use std::{
    collections::HashSet,
    io::{BufWriter, Cursor, copy, stdout},
    path::{Path, PathBuf},
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Parser)]
#[command(name = "dump-pdf", about = "Dump PDF file structure and contents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "dump stream content to stdout")]
    Stream {
        #[arg(short, long, help = "PDF file to dump")]
        filename: PathBuf,

        #[arg(short, long, help = "Password for encrypted PDF file")]
        password: Option<String>,

        #[arg(help = "object ID to dump")]
        object_id: u32,

        #[arg(long, help = "Skip decoding stream content")]
        raw: bool,

        #[arg(long, help = "Assume stream is image, decode and convert to PNG")]
        png: bool,
    },

    #[command(about = "dump page content to stdout")]
    Page {
        #[arg(short, long, help = "PDF file to dump")]
        filename: PathBuf,

        #[arg(short, long, help = "Password for encrypted PDF file")]
        password: Option<String>,

        #[arg(long, help = "display total page numbers")]
        pages: bool,

        #[arg(long, help = "display page object ID")]
        id: bool,

        #[arg(long, help = "Render page to PNG")]
        png: bool,

        #[arg(long, help = "Zoom factor for PNG rendering, default: 1.75")]
        zoom: Option<String>,

        #[arg(long, help = "Do not apply CropBox")]
        no_crop: bool,

        #[arg(long, help = "Stop render after <steps> graphic steps")]
        steps: Option<String>,

        #[arg(help = "page number (start from zero) to dump")]
        page_no: Option<String>,
    },

    #[command(about = "dump pdf object by id")]
    Object {
        #[arg(short, long, help = "PDF file to dump")]
        filename: PathBuf,

        #[arg(short, long, help = "Password for encrypted PDF file")]
        password: Option<String>,

        #[arg(help = "object ID to dump")]
        object_id: u32,
    },
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
        let mut contents = contents.as_slice();
        for op in parse_operations(&mut contents) {
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

            id_wait_scanned.extend(
                obj.iter_values()
                    .filter_map(|o| o.reference().ok().map(RuntimeObjectId::from)),
            );
        }
    }

    Ok(())
}

#[report]
fn main() -> Result<()> {
    colog::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Stream {
            filename,
            password,
            object_id,
            raw,
            png,
        } => dump_stream(
            &filename,
            password.as_deref().unwrap_or(""),
            object_id,
            raw,
            png,
        ),
        Commands::Page {
            filename,
            password,
            page_no,
            pages,
            id,
            png,
            steps,
            zoom,
            no_crop,
        } => dump_page(&DumpPageArgs {
            path: &filename,
            password: password.as_deref().unwrap_or(""),
            page_no: page_no.and_then(|s| s.parse().ok()),
            show_total_pages: pages,
            show_page_id: id,
            to_png: png,
            steps: steps.and_then(|s| s.parse().ok()),
            zoom: zoom.and_then(|s| s.parse().ok()),
            no_crop,
        }),
        Commands::Object {
            filename,
            password,
            object_id,
        } => dump_object(&filename, password.as_deref().unwrap_or(""), object_id),
    }
}
