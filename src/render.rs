use anyhow::{Context, Result};
use crate::InputType;
use std::path::PathBuf;
use crate::RpixContext;
#[cfg(feature = "html")]
use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "html")]
use directories::ProjectDirs;
#[cfg(feature = "html")]
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
#[cfg(feature = "html")]
use headless_chrome::{Browser, LaunchOptions};
use image::{DynamicImage, GenericImage, RgbaImage};
#[cfg(feature = "pdf")]
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

#[cfg(test)]
mod tests_render;

#[cfg(feature = "svg")]
pub fn render_svg(data: &[u8]) -> Result<DynamicImage> {
    // load system fonts
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    // configure options
    let opt = usvg::Options {
        fontdb: std::sync::Arc::new(fontdb),
        ..Default::default()
    };

    // parse the SVG
    let tree = usvg::Tree::from_data(data, &opt).context("Failed to parse SVG")?;

    // pixel buffer
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    // convert to DynamicImage
    let buffer = RgbaImage::from_raw(size.width(), size.height(), pixmap.data().to_vec())
        .ok_or_else(|| anyhow::anyhow!("Failed buffer conversion"))?;

    Ok(DynamicImage::ImageRgba8(buffer))
}

#[cfg(feature = "pdf")]
pub fn render_pdf(
    data: &[u8],
    conf_w: Option<u32>,
    term_width: u32,
    page_indices: Option<Vec<u16>>,
) -> Result<DynamicImage> {
    let width = conf_w
        .unwrap_or(term_width)
        .try_into()
        .context("Failed to convert width to i32")?;

    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
            .or_else(|_| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./pdfium/"))
            })
            .or_else(|_| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                    "/opt/homebrew/lib",
                ))
            })
            .or_else(|_| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                    "/usr/local/lib",
                ))
            })
            .or_else(|_| Pdfium::bind_to_system_library())?,
    );

    let config = PdfRenderConfig::new()
        .set_target_width(width)
        .render_form_data(true);
    let document = pdfium.load_pdf_from_byte_slice(data, None)?;
    let pages = document.pages();
    let n_pages = pages.len();
    let selected_indices = if let Some(page_indices) = page_indices {
        if page_indices.iter().any(|&i| i >= n_pages) {
            anyhow::bail!("Page index out of range (must be <= {})", n_pages);
        }
        page_indices
    } else {
        (0..n_pages).collect()
    };

    let mut images: Vec<RgbaImage> = Vec::new();
    for page_index in selected_indices.iter() {
        let page = pages
            .get(*page_index)
            .context(format!("Failed to get page {}", page_index))?;
        let bitmap = page.render_with_config(&config)?;
        let image = bitmap.as_image().to_rgba8();
        images.push(image);
    }
    if images.is_empty() {
        anyhow::bail!("No pages found in PDF");
    }
    let max_width = images.iter().map(|img| img.width()).max().unwrap();
    let total_height = images.iter().map(|img| img.height()).sum::<u32>();
    let mut combined = RgbaImage::new(max_width, total_height);
    let mut current_y = 0;
    for img in images {
        combined.copy_from(&img, 0, current_y)?;
        current_y += img.height();
    }
    Ok(DynamicImage::ImageRgba8(combined))
}

#[cfg(feature = "html")]
fn chromium_path() -> PathBuf {
    let proj_dirs =
        ProjectDirs::from("org", "example", "rpix").expect("Could not determine XDG data dir");
    proj_dirs.data_dir().join("chromium")
}

#[cfg(feature = "html")]
fn is_url(s: &[u8]) -> bool {
    s.starts_with(b"http://") || s.starts_with(b"https://") || s.starts_with(b"file://")
}

#[cfg(feature = "html")]
fn is_url_str(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://")
}

#[cfg(feature = "html")]
pub fn is_html(ctx: &RpixContext, extension: &str, s: &[u8]) -> bool {
    ctx.input_type == InputType::Html || extension == "html" || extension == "htm" || is_url(s)
}

#[cfg(feature = "html")]
pub fn render_html_chrome(data: &[u8]) -> Result<DynamicImage> {
    let data_str = std::str::from_utf8(data)?;
    let url: String = if is_url_str(data_str) {
        data_str.to_owned()
    } else {
        let path = PathBuf::from(data_str);
        if path.exists() {
            let absolute_path = path.canonicalize()?;
            format!("file://{}", absolute_path.display())
        } else {
            format!(
                "data:text/html;base64,{}",
                general_purpose::STANDARD.encode(data)
            )
        }
    };

    let user_data_dir = chromium_path();
    std::fs::create_dir_all(&user_data_dir)?;
    let browser = Browser::new(LaunchOptions {
        headless: true,
        path: None,
        user_data_dir: Some(user_data_dir),
        ..Default::default()
    })?;
    let tab = browser.new_tab()?;
    tab.navigate_to(&url)?;
    tab.wait_for_element("body")?;
    let png_data = tab.capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)?;
    Ok(image::load_from_memory(&png_data)?)
}

#[cfg(feature = "office")]
pub fn render_office(
    data: &[u8],
    extension: &str,
    pages: Option<Vec<u16>>,
) -> Result<DynamicImage> {
    // Write data to temporary file because office libs usually need seekable files
    let mut temp = tempfile::NamedTempFile::new()?;
    std::io::Write::write_all(&mut temp, data)?;
    let path = temp.path();

    let mut html_content = String::from(
        "<html><body style='background:white; color:black; font-family: sans-serif;'>",
    );

    if extension == "xlsx" {
        // Excel handling
        let mut workbook: Xlsx<_> = calamine::open_workbook(path).context("Cannot open XLSX")?;

        // Simple heuristic: if specific pages (worksheets) requested, use indices
        let sheets = workbook.sheet_names().to_owned();
        let selected_sheets = if let Some(indices) = pages {
            indices
                .iter()
                .filter_map(|&i| sheets.get(i as usize).map(|s| s.clone()))
                .collect()
        } else {
            // Default to first sheet
            sheets.first().map(|s| vec![s.clone()]).unwrap_or_default()
        };

        for s_name in selected_sheets {
            html_content.push_str(&format!(
                "<h1>{}</h1><table border='1' style='border-collapse: collapse;'>",
                s_name
            ));
            if let Ok(range) = workbook.worksheet_range(&s_name) {
                for row in range.rows() {
                    html_content.push_str("<tr>");
                    for cell in row {
                        html_content.push_str(&format!("<td style='padding: 4px;'>{}</td>", cell));
                    }
                    html_content.push_str("</tr>");
                }
            }
            html_content.push_str("</table><br/>");
        }
    } else if extension == "pptx" {
        // Rudimentary PPTX: Iterate ppt/slides/slideX.xml in the zip
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut slide_indices = Vec::new();
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let name = file.name();
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                // extract number
                let num_str: String = name.chars().filter(|c| c.is_numeric()).collect();
                if let Ok(num) = num_str.parse::<u16>() {
                    slide_indices.push(num);
                }
            }
        }
        slide_indices.sort();

        let targets = if let Some(p) = pages {
            p
        } else {
            slide_indices
        };

        for i in targets {
            let entry_name = format!("ppt/slides/slide{}.xml", i); // standard naming usually 1-indexed in filename
                                                                   // Try to find exact entry
            let mut zip_file = match archive.by_name(&entry_name) {
                Ok(f) => f,
                Err(_) => {
                    // Try to fallback to 1-based index logic if i came from parsing
                    if let Ok(f) = archive.by_name(&format!("ppt/slides/slide{}.xml", i + 1)) {
                        f
                    } else {
                        continue;
                    }
                }
            };

            let mut xml_content = String::new();
            zip_file.read_to_string(&mut xml_content)?;

            // Very naive text extraction from XML
            let mut reader = XmlReader::from_str(&xml_content);
            reader.trim_text(true);
            let mut txt = String::new();
            let mut buf = Vec::new();

            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Text(e)) => txt.push_str(&e.unescape().unwrap_or_default()),
                    Ok(Event::Eof) => break,
                    _ => (),
                }
                buf.clear();
            }

            html_content.push_str(&format!("<div style='border: 1px solid black; padding: 20px; margin: 20px; min-height: 400px;'><h2>Slide {}</h2><p>{}</p></div>", i, txt));
        }
    } else if extension == "docx" {
        // Rudimentary DOCX
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        if let Ok(mut doc) = archive.by_name("word/document.xml") {
            let mut xml_content = String::new();
            doc.read_to_string(&mut xml_content)?;

            let mut reader = XmlReader::from_str(&xml_content);
            reader.trim_text(true);

            html_content.push_str("<div style='padding: 40px; max-width: 800px;'>");
            let mut buf = Vec::new();
            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Text(e)) => {
                        let t = e.unescape().unwrap_or_default();
                        html_content.push_str(&format!("{} ", t));
                    }
                    Ok(Event::End(ref e)) if e.name().as_ref() == b"p" => {
                        html_content.push_str("<br/><br/>");
                    }
                    Ok(Event::Eof) => break,
                    _ => (),
                }
                buf.clear();
            }
            html_content.push_str("</div>");
        }
    }

    html_content.push_str("</body></html>");

    // Pass generated HTML to chrome renderer
    render_html_chrome(html_content.as_bytes())
}
