use anyhow::{Context, Result};

use image::{DynamicImage, Rgba, RgbaImage};

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

mod render;

use render::*;

#[cfg(test)]
mod tests_lib;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputType {
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

pub struct RpixContext {
    pub input_type: InputType,
    pub conf_w: Option<u32>,
    pub conf_h: Option<u32>,
    pub term_width: u32,
    pub term_height: u32,
    pub page_indices: Option<Vec<u16>>,
}

pub fn get_term_size() -> (u32, u32) {
    let mut width = 800; // ultimate fallback
    let mut height = 400; // ultimate fallback
    if let Ok(size) = crossterm::terminal::window_size() {
        // try raw pixels
        // fallback: if 0 pixels, estimate based on columns
        let cols = size.columns as u32;
        let rows = size.rows as u32;

        if size.width > 0 {
            width = size.width as u32;
        } else if cols > 0 {
            width = cols * 10;
        }
        // if possible adjust for the new prompt line and the empty line after the image
        if size.height > 0 {
            height = size.height as u32;
            if cols > 0 {
                height = height * (rows - 2) / rows;
            }
        } else if rows > 0 {
            height = (rows - 2) * 20;
        }
    }
    (width, height)
}

pub fn parse_color(color: &str) -> Result<Rgba<u8>> {
    let color = color.trim_start_matches('#'); // Allow #FFFFFF
    if color.len() != 6 {
        return Err(anyhow::anyhow!("Invalid color format: {}", color));
    }
    let r = u8::from_str_radix(&color[0..2], 16)?;
    let g = u8::from_str_radix(&color[2..4], 16)?;
    let b = u8::from_str_radix(&color[4..6], 16)?;
    Ok(Rgba([r, g, b, 255]))
}

pub fn add_background(img: &DynamicImage, color: &Rgba<u8>) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut bg = RgbaImage::new(w, h);

    let bg_r = color[0] as u32;
    let bg_g = color[1] as u32;
    let bg_b = color[2] as u32;

    for (src, dst) in rgba.pixels().zip(bg.pixels_mut()) {
        let alpha = src[3] as u32;

        // if opaque, just copy it
        if alpha == 255 {
            *dst = *src;
            continue;
        }

        // if transparent, use background color
        if alpha == 0 {
            *dst = *color;
            continue;
        }

        // manual blending: src * alpha + bg * (1 - alpha)
        let inv_alpha = 255 - alpha;

        let r = (src[0] as u32 * alpha + bg_r * inv_alpha) / 255;
        let g = (src[1] as u32 * alpha + bg_g * inv_alpha) / 255;
        let b = (src[2] as u32 * alpha + bg_b * inv_alpha) / 255;

        *dst = Rgba([r as u8, g as u8, b as u8, 255]);
    }

    DynamicImage::ImageRgba8(bg)
}

pub fn calculate_dimensions(
    img_dims: (u32, u32),
    conf_size: (Option<u32>, Option<u32>),
    fullwidth: bool,
    fullheight: bool,
    resize: bool,
    noresize: bool,
    term_size: (u32, u32),
) -> (u32, u32) {
    let (orig_w, orig_h) = (img_dims.0 as f64, img_dims.1 as f64);
    let mut width = conf_size.0.unwrap_or(0) as f64;
    let mut height = conf_size.1.unwrap_or(0) as f64;

    let mut use_resize = resize;
    let mut use_fullwidth = fullwidth;
    let mut use_fullheight = fullheight;

    // if neither noresize nor fullwidth nor fullheight is set,
    // then resize if the image is larger than the terminal
    if !noresize
        && !use_fullwidth
        && !use_fullheight
        && ((orig_w > term_size.0.into() && term_size.0 > 0)
            || (orig_h > term_size.1.into() && term_size.1 > 0))
    {
        use_resize = true;
    }

    if use_resize {
        let aspect_w = orig_w / orig_h;
        let aspect_h = orig_h / orig_w;
        if aspect_w > aspect_h {
            use_fullwidth = true;
        } else {
            use_fullheight = true;
        }
    }

    // if width or height is set, use it
    if width > 0.0 && height == 0.0 {
        height = orig_h * (width / orig_w);
    } else if height > 0.0 && width == 0.0 {
        width = orig_w * (height / orig_h);
    // use full terminal width, scale height by aspect ratio
    } else if use_fullwidth {
        width = term_size.0.into();
        height = orig_h * (width / orig_w);
    // use full terminal height, scale width by aspect ratio
    } else if use_fullheight {
        height = term_size.1.into();
        width = orig_w * (height / orig_h);
    // use original size
    } else {
        width = orig_w;
        height = orig_h;
    }
    (width.round() as u32, height.round() as u32)
}

// Parse a 1-indexed pages string to 0-indexed vector
pub fn parse_pages(pages: &str) -> Result<Option<Vec<u16>>> {
    let mut result = Vec::new();
    for part in pages.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.contains('-') {
            let mut parts = part.split('-');
            let start = parts
                .next()
                .context("Invalid page range")?
                .trim()
                .parse::<u16>()?;
            let end = parts
                .next()
                .context("Invalid page range")?
                .trim()
                .parse::<u16>()?;
            if start < 1 || end <= start {
                return Err(anyhow::anyhow!(
                    "Page range must start >= 1 and end > start"
                ));
            }
            for i in start..=end {
                result.push(i - 1);
            }
        } else {
            let index = part.parse::<u16>().context("Invalid page index")?;
            if index < 1 {
                return Err(anyhow::anyhow!("Page index must be >= 1"));
            }
            result.push(index - 1);
        }
    }
    if result.is_empty() {
        return Ok(None);
    }
    // sort and deduplicate
    result.sort();
    result.dedup();
    Ok(Some(result))
}

pub fn load_file(ctx: &RpixContext, path: &PathBuf) -> Result<DynamicImage> {
    let extension = path.extension().map_or("", |e| e.to_str().unwrap_or(""));

    #[cfg(feature = "html")]
    {
        let path_bytes = path.to_str().unwrap().as_bytes();
        if is_html(ctx, extension, path_bytes) {
            return render_html_chrome(path_bytes);
        }
    }

    let mut file = File::open(path).context("Failed to open file")?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    load_data(ctx, &data, extension)
}

pub fn load_data(ctx: &RpixContext, data: &[u8], extension: &str) -> Result<DynamicImage> {
    if ctx.input_type == InputType::Text {
        // not implemented yet
        return Err(anyhow::anyhow!("Text input not implemented yet"));
    }

    if ctx.input_type == InputType::Image {
        return image::load_from_memory(data).context("Failed to load image");
    }

    #[cfg(feature = "svg")]
    if ctx.input_type == InputType::Svg
        || extension == "svg"
        || data.starts_with(b"<svg")
        || data.starts_with(b"<?xml")
    {
        return render_svg(data);
    }

    #[cfg(feature = "pdf")]
    if ctx.input_type == InputType::Pdf || extension == "pdf" || data.starts_with(b"%PDF") {
        return render_pdf(data, ctx.conf_w, ctx.term_width, ctx.page_indices.clone());
    }

    #[cfg(feature = "office")]
    if ctx.input_type == InputType::Office || ["xlsx", "docx", "pptx"].contains(&extension) {
        return render_office(data, extension, ctx.page_indices.clone());
    }

    #[cfg(feature = "html")]
    if is_html(ctx, extension, data)
        || data.starts_with(b"<html")
        || data.starts_with(b"<!DOCTYPE html")
    {
        return render_html_chrome(data);
    }

    // fallback for input_type == InputType::Auto
    match image::load_from_memory(data) {
        Ok(img) => Ok(img),
        Err(err) => {
            if let Ok(text) = std::str::from_utf8(data) {
                let path_str = text.trim();
                let path = PathBuf::from(path_str);
                if path.exists() && path.is_file() {
                    return load_file(ctx, &path);
                }
            }
            Err(anyhow::anyhow!("Failed to decode input: {}", err))
        }
    }
}
