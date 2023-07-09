use std::{
    io::{copy, stdout, Cursor},
};

use anyhow::Result as AnyResult;

use clap::{arg, Command};
use image::ImageOutputFormat;
use pdf2docx::{
    file::File,
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
}

fn dump_stream(path: &str, id: u32, raw: bool, as_image: bool, as_png: bool) -> AnyResult<()> {
    let buf = std::fs::read(path).unwrap();
    let (_f, mut resolver) =
        File::parse(&buf[..]).unwrap_or_else(|_| panic!("failed to parse {:?}", path));
    let obj = resolver.resolve(id)?;
    let png_buffer;
    match obj {
        Object::Stream(s) => {
            let decoded;
            let mut buf = if raw {
                s.1
            } else {
                decoded = s.decode(as_image)?;
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
            copy(&mut buf, &mut stdout())?;
        }
        _ => eprintln!("object is not a stream"),
    };
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
        _ => todo!(),
    }
}
