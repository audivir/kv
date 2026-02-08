use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use clap::{Parser, ValueEnum};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageEncoder};
use rpix::*;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use std::process::Command;
const CHUNK_SIZE: usize = 4096;

#[cfg(test)]
mod tests_main;

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum Mode {
    Png,
    Zlib,
    Raw,
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum InputTypeOption {
    Auto,
    Image,
    Text,
    #[cfg(feature = "svg")]
    Svg,
    #[cfg(feature = "pdf")]
    Pdf,
    #[cfg(feature = "html")]
    Html,
    #[cfg(feature = "office")]
    Office,
}

impl From<InputTypeOption> for InputType {
    fn from(arg: InputTypeOption) -> Self {
        match arg {
            InputTypeOption::Auto => InputType::Auto,
            InputTypeOption::Image => InputType::Image,
            InputTypeOption::Text => InputType::Text,
            #[cfg(feature = "svg")]
            InputTypeOption::Svg => InputType::Svg,
            #[cfg(feature = "pdf")]
            InputTypeOption::Pdf => InputType::Pdf,
            #[cfg(feature = "html")]
            InputTypeOption::Html => InputType::Html,
            #[cfg(feature = "office")]
            InputTypeOption::Office => InputType::Office,
        }
    }
}

type TempAndFinalOption = Option<(NamedTempFile, PathBuf)>;
/// A image viewer for the Kitty Terminal Graphics Protocol.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Config {
    /// Input files
    #[arg(name = "FILES")]
    files: Vec<PathBuf>,

    /// Specify image width
    #[arg(
        short = 'w',
        long,
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    width: Option<u32>,

    /// Specify image height
    #[arg(
        short = 'H', // else conflicts with --help
        long,
        conflicts_with = "width",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    height: Option<u32>,

    /// Resize image to fill terminal width
    #[arg(
        short = 'f',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullheight",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    fullwidth: bool,

    /// Resize image to fill terminal height
    #[arg(
        short = 'F',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    fullheight: bool,

    /// Resize image to fill terminal
    #[arg(
        short = 'r',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "noresize"
    )]
    resize: bool,

    /// Disable automatic resizing
    #[arg(
        short = 'n',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "resize"
    )]
    noresize: bool,

    /// Add background if image is transparent
    #[arg(short = 'b', long)]
    background: bool,

    /// Background color as hex string
    #[arg(short = 'C', long, default_value = "FFFFFF", requires = "background")]
    color: String,

    /// Transmission mode
    #[arg(short = 'm', long, value_enum, default_value_t = Mode::Png)]
    mode: Mode,

    /// Output to file as png, instead of kitty
    #[arg(short = 'o', long, conflicts_with = "mode")]
    output: Option<String>,

    /// Overwrite existing output file
    #[arg(short = 'x', long, requires = "output")]
    overwrite: bool,

    /// Input type
    #[arg(short = 'i', long, value_enum, default_value_t = InputTypeOption::Auto, conflicts_with = "pages")]
    input: InputTypeOption,

    /// Select which PDF pages to render, forces input type to pdf (e.g. "1-3,34")
    #[arg(short = 'P', long, conflicts_with = "input")]
    pages: Option<String>,

    /// Print file name
    #[arg(short = 'p', long)]
    printname: bool,

    /// Force tty (ignore stdin check)
    #[arg(short = 't', long)]
    tty: bool,

    /// Clear terminal (does not print image)
    #[arg(short = 'c', long)]
    clear: bool,
}

fn render_image(
    mut writer: impl Write,
    img: DynamicImage,
    conf: &Config,
    term_size: (u32, u32),
) -> Result<()> {
    let (w, h) = calculate_dimensions(
        img.dimensions(),
        (conf.width, conf.height),
        conf.fullwidth,
        conf.fullheight,
        conf.resize,
        conf.noresize,
        term_size,
    );
    let mut final_img = img;

    if w != 0 && h != 0 && (w != final_img.width() || h != final_img.height()) {
        final_img = final_img.resize_exact(w, h, FilterType::Triangle);
    }

    if conf.background {
        match parse_color(&conf.color) {
            Ok(color) => final_img = add_background(&final_img, &color),
            Err(e) => return Err(e),
        }
    }

    let payload: Vec<u8>;
    let header: String;

    if conf.output.is_some() || conf.mode == Mode::Png {
        // encode as png
        let mut buffer = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buffer);
        let (width, height) = final_img.dimensions();
        let color_type = final_img.color();

        encoder.write_image(final_img.as_bytes(), width, height, color_type.into())?;

        if conf.output.is_some() {
            // write the raw bytes
            writer.write_all(&buffer)?;
            return Ok(());
        }

        payload = buffer;

        // f=100 (PNG), no width/height data
        header = "a=T,f=100,".to_string();
    } else {
        let (width, height) = final_img.dimensions();
        let raw_bytes = final_img.to_rgba8().into_raw();

        if conf.mode == Mode::Zlib {
            // compress with zlib
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&raw_bytes)?;
            payload = encoder.finish()?;
        } else {
            // Raw
            payload = raw_bytes;
        }

        // f=32 (RGBA), o=z (compressed)
        header = format!("a=T,f=32,s={},v={},o=z,", width, height);
    }

    // base64 encode payload
    let encoded = general_purpose::STANDARD.encode(&payload);

    let total_len = encoded.len();
    let mut pos = 0;
    let mut is_first = true;

    while pos < total_len {
        let end = std::cmp::min(pos + CHUNK_SIZE, total_len);
        let chunk = &encoded[pos..end];
        let more = if end < total_len { 1 } else { 0 };

        write!(writer, "\x1b_G")?;
        if is_first {
            write!(writer, "{}", header)?;
        }
        write!(writer, "m={};", more)?;
        writer.write_all(chunk.as_bytes())?;
        write!(writer, "\x1b\\")?;

        pos = end;
        is_first = false;
    }
    writeln!(writer)?;
    Ok(())
}

fn fallback_viewer_data(data : &[u8]) -> Result<()> {
    // pipe the data to bat
    let mut viewer = if let Ok(bat) = Command::new("bat")
        .arg("--paging=never")
        .arg("--style=plain")
        .arg("-")
        .spawn() {
        bat
    } else {
        // fallback cat
        let cat = Command::new("cat").arg("-").spawn()?;
        cat
    };
    viewer.stdin.as_mut().unwrap().write_all(data)?;
    viewer.wait()?;
    Ok(())
}

fn fallback_viewer(path: &PathBuf) -> Result<()> {
    // try 'bat' with --paging=never
    if Command::new("bat")
        .arg("--paging=never")
        .arg("--style=plain")
        .arg(path)
        .status()
        .is_ok()
    {
        return Ok(());
    }

    // fallback to 'cat'
    Command::new("cat").arg(path).status()?;
    Ok(())
}

fn run(
    mut writer: impl Write,
    mut err_writer: impl Write,
    mut reader: impl Read,
    conf: Config,
    term_size: (u32, u32),
    is_input_available: bool,
) -> Result<i32> {
    if conf.clear {
        write!(writer, "\x1b_Ga=d\x1b\\")?;
        return Ok(0);
    }

    // If -t is passed, we ignore stdin even if input is available
    let use_stdin = is_input_available && !conf.tty;

    if conf.output.is_some() && !use_stdin && conf.files.len() > 1 {
        writeln!(
            err_writer,
            "Error: Cannot specify multiple files with --output"
        )?;
        return Ok(1);
    }

    let page_indices = if let Some(pages) = conf.pages.clone() {
        if !use_stdin && conf.files.len() > 1 {
            writeln!(
                err_writer,
                "Error: Cannot specify multiple files with --pages"
            )?;
            return Ok(1);
        }
        let page_indices = if let Ok(pages) = parse_pages(&pages) {
            pages
        } else {
            writeln!(err_writer, "Error: Invalid page range")?;
            return Ok(1);
        };
        page_indices
    } else {
        None
    };

    let ctx = RpixContext {
        input_type: conf.input.clone().into(),
        conf_w: conf.width,
        conf_h: conf.height,
        term_width: term_size.0,
        term_height: term_size.1,
        page_indices,
    };

    if use_stdin {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        
        match load_data(&ctx, &data, "") {
            Ok(img) => {
                if conf.printname {
                    writeln!(err_writer, "stdin")?;
                }
                render_image(writer, img, &conf, term_size)?;
            }
            Err(e) => {
                // If data is not an image, try to print as text
                if !conf.output.is_some() && e.to_string().contains("Failed to decode input") {
                     if let Ok(text) = String::from_utf8(data) {
                        fallback_viewer_data(&text.as_bytes())?;
                        return Ok(0);
                     }
                }
                writeln!(err_writer, "Error decoding stdin: {}", e)?;
                return Ok(1);
            }
        }
    } else if !conf.files.is_empty() {
        let mut exit_code = 0;
        for path in &conf.files {
            if conf.printname {
                writeln!(err_writer, "{}", path.display())?;
            }
            match load_file(&ctx, path) {
                Ok(img) => {
                    if let Err(e) = render_image(&mut writer, img, &conf, term_size) {
                        writeln!(err_writer, "Error rendering {}: {}", path.display(), e)?;
                        exit_code = 1;
                    }
                }
                Err(e) => {
                    // FALLBACK: If not an image, try fallback viewer (bat/cat)
                    if !conf.output.is_some() && e.to_string().contains("Failed to decode") {
                        if let Err(fe) = fallback_viewer(path) {
                            writeln!(err_writer, "Error loading {}: {} (Fallback failed: {})", path.display(), e, fe)?;
                            exit_code = 1;
                        }
                    } else {
                        writeln!(err_writer, "Error loading {}: {}", path.display(), e)?;
                        exit_code = 1;
                    }
                }
            }
        }
        return Ok(exit_code);
    } else {
        writeln!(
            err_writer,
            "Error: No input files provided and no data piped to stdin."
        )?;
        return Ok(1);
    }

    Ok(0)
}

fn prepare_writer(
    output: Option<String>,
    overwrite: bool,
) -> Result<(Box<dyn Write>, TempAndFinalOption)> {
    match output {
        Some(path_str) => {
            let path = PathBuf::from(path_str);
            // canonicalize path
            let absolute_path = path.canonicalize()?;
            let parent = absolute_path.parent().context("Invalid output path")?;

            if !parent.exists() {
                anyhow::bail!("Output directory does not exist: {}", parent.display());
            }

            if absolute_path.exists() && !overwrite {
                anyhow::bail!(
                    "Output file already exists: {} (use --overwrite)",
                    path.display()
                );
            }

            let tempfile = NamedTempFile::new_in(parent).context(format!(
                "Failed to create temp file in {}",
                parent.display()
            ))?;

            let writer: Box<dyn Write> = Box::new(
                tempfile
                    .as_file()
                    .try_clone()
                    .context("Failed to clone temp file")?,
            );
            Ok((writer, Some((tempfile, absolute_path))))
        }

        None => Ok((Box::new(io::stdout()), None)),
    }
}

fn main() -> Result<()> {
    let conf = Config::parse();
    let term_size = get_term_size();

    // Detect TTY status
    let is_input_available = atty::isnt(atty::Stream::Stdin);

    let (writer, temp_output) = prepare_writer(conf.output.clone(), conf.overwrite)?;

    let code = run(
        writer,
        io::stderr(),
        io::stdin(),
        conf,
        term_size,
        is_input_available,
    )?;

    // Commit temp file only on success
    if let Some((tempfile, final_path)) = temp_output {
        if code == 0 {
            tempfile.persist(final_path)?;
        }
    }

    std::process::exit(code);
}
