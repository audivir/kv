use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose};
use bat::{Input, PrettyPrinter};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, GenericImageView, ImageEncoder};
use std::io::{Cursor, Write};
use std::path::PathBuf;

const KITTY_CHUNK_SIZE: usize = 4096;
const INPUT_CHUNK_SIZE: usize = (KITTY_CHUNK_SIZE * 3) / 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Png,
    Zlib,
    Raw,
}

pub enum PrinterInput {
    File(PathBuf),
    Data(Vec<u8>),
}

/// Terminal height in rows and pixels, used to derive the cell height.
pub type TermCells = (u32, u32);

/// Reads the terminal height as (rows, pixels), if the terminal reports it.
fn terminal_cells() -> Option<TermCells> {
    let size = crossterm::terminal::window_size().ok()?;
    (size.rows > 0 && size.height > 0).then_some((size.rows as u32, size.height as u32))
}

/// Returns the number of terminal rows covered by an image of the given pixel height.
fn image_rows(height: u32, (rows, px_height): TermCells) -> u32 {
    let cell_h = px_height as f64 / rows as f64;
    ((height as f64 / cell_h).ceil() as u32).max(1)
}

pub fn send_image(
    writer: &mut dyn Write,
    img: DynamicImage,
    output: Option<String>,
    mode: Mode,
) -> Result<()> {
    send_image_with_cells(writer, img, output, mode, terminal_cells())
}

/// Sends an image to the terminal, using `cells` to place the cursor below the image.
///
/// Without `cells`, the terminal moves the cursor itself and a single newline is written.
pub fn send_image_with_cells(
    writer: &mut dyn Write,
    img: DynamicImage,
    output: Option<String>,
    mode: Mode,
    cells: Option<TermCells>,
) -> Result<()> {
    let payload = match mode {
        Mode::Png => {
            let mut buffer = Vec::new();
            let (width, height) = img.dimensions();
            let color_type = img.color();

            // scope ensures flush
            {
                let encoder = PngEncoder::new(&mut buffer);
                encoder
                    .write_image(img.as_bytes(), width, height, color_type.into())
                    .context("Failed to encode image to PNG")?;
            }
            buffer
        }
        Mode::Raw => img.to_rgba8().into_raw(),
        Mode::Zlib => {
            let raw_bytes = img.to_rgba8().into_raw();
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&raw_bytes)?;
            encoder.finish().context("Failed to compress image data")?
        }
    };

    if output.is_some() {
        writer.write_all(&payload)?;
        return Ok(());
    }
    // Png: a=T,f=100
    // Raw: a=T,f=32,s={w},v={h} (+ o=z if zlib)
    let (width, height) = img.dimensions();
    // with C=1 the terminal keeps the cursor in place and the newlines below move it past
    // the image.
    // for tall images r= pins the row count, so the terminal and the newline count cannot
    // disagree when the reported pixel height includes padding.
    let rows = cells.map(|c| image_rows(height, c));
    let cursor = match rows {
        Some(rows) if rows >= 8 => format!(",C=1,r={rows}"),
        Some(_) => ",C=1".to_string(),
        None => String::new(),
    };
    let header = match mode {
        Mode::Png => format!("a=T,f=100{cursor}"),
        Mode::Zlib => format!("a=T,f=32,s={},v={},o=z{cursor}", width, height),
        Mode::Raw => format!("a=T,f=32,s={},v={}{cursor}", width, height),
    };

    let total_len = payload.len();
    let mut offset = 0;

    // reusable buffer
    let mut b64_buffer = String::with_capacity(KITTY_CHUNK_SIZE + 4);

    while offset < total_len {
        let end = (offset + INPUT_CHUNK_SIZE).min(total_len);
        let chunk_data = &payload[offset..end];

        // encode chunk to base64
        b64_buffer.clear();
        general_purpose::STANDARD.encode_string(chunk_data, &mut b64_buffer);

        let more = if end < total_len { 1 } else { 0 };

        write!(writer, "\x1b_G")?;

        // send control header only on the first chunk
        if offset == 0 {
            write!(writer, "{},", header)?;
        }

        // send payload
        write!(writer, "m={};", more)?;
        writer.write_all(b64_buffer.as_bytes())?;

        // end escape sequence
        write!(writer, "\x1b\\")?;

        offset = end;
    }

    // the cursor movement of the terminal does not scroll reliably for images taller than
    // the viewport (seen in Ghostty), so the newlines are written here.
    for _ in 0..rows.unwrap_or(1) {
        writeln!(writer)?;
    }
    writer.flush()?;

    Ok(())
}

pub fn pretty_print(
    writer: &mut dyn Write,
    input: PrinterInput,
    language: Option<&str>,
    theme: &str,
    newline: bool,
) -> Result<()> {
    let mut printer = PrettyPrinter::new();

    match input {
        PrinterInput::File(path) => {
            printer.input_file(path);
        }
        PrinterInput::Data(data) => {
            // requires a reader
            printer.input(Input::from_reader(Box::new(Cursor::new(data))));
        }
    };

    if let Some(lang) = language {
        printer.language(lang);
    }
    printer.theme(theme);

    // capture output to handle the optional newline
    let mut output_str = String::new();
    printer
        .print_with_writer(Some(&mut output_str))
        .map_err(|e| anyhow::anyhow!("Syntax highlighting failed: {}", e))?;

    if newline && !output_str.ends_with('\n') {
        output_str.push('\n');
    }

    writer.write_all(output_str.as_bytes())?;
    writer.flush()?;
    Ok(())
}
