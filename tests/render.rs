use image::{DynamicImage, GenericImageView, Rgba};
use kv::*;
use rstest::rstest;
use std::sync::Mutex;

// cargo test runs cases in parallel by default; serialize Chrome launches so a
// resource-limited CI runner isn't spawning several full browser processes at once.
static CHROME_TEST_LOCK: Mutex<()> = Mutex::new(());

const SVG_DATA: &[u8] = include_bytes!("fixtures/test.svg");
const PDF_DATA: &[u8] = include_bytes!("fixtures/test.pdf");
const HTML_DATA: &[u8] = include_bytes!("fixtures/test.html");
const RANDOM_DATA: &[u8] = include_bytes!("fixtures/test.random");
const DOCX_DATA: &[u8] = include_bytes!("fixtures/test.docx");
const XLSX_DATA: &[u8] = include_bytes!("fixtures/test.xlsx");
const DOCX_WITH_IMAGE_DATA: &[u8] = include_bytes!("fixtures/test_with_image.docx");
const MARKDOWN_DATA: &[u8] = include_bytes!("fixtures/test.md");

// Kitty graphics protocol escape sequences start with this prefix.
const KITTY_IMAGE_PREFIX: &str = "\x1b_Ga=T";

fn ctx_with(resize_mode: ResizeMode, term_size: (u32, u32), page_indices: Option<Vec<u16>>) -> KvContext {
    KvContext {
        input_type: InputType::Auto,
        resize_mode,
        term_size,
        page_indices,
        cache_mode: CacheMode::Disabled,
        background_color: None,
        render_as_pdf: false,
    }
}

#[test]
fn test_render_svg() {
    let ctx = ctx_with(ResizeMode::Original, (100, 50), None);
    let result = render_svg(&ctx, SVG_DATA);
    assert!(result.is_ok(), "SVG generation failed");

    let img = result.unwrap();
    assert_eq!(img.width(), 1);
    assert_eq!(img.height(), 1);

    let pixel = img.get_pixel(0, 0);
    assert_eq!(pixel, Rgba([102, 102, 102, 255]));
}

#[test]
fn test_render_svg_invalid() {
    let svg_data = br#"<svg>invalid"#;
    let ctx = ctx_with(ResizeMode::Original, (100, 50), None);

    let result = render_svg(&ctx, svg_data);
    assert!(result.is_err(), "SVG generation failed");
}

#[rstest]
#[case(None, 100, None, 31)] // ClipTerminal: page is taller than wide, so height (50) binds
#[case(None, 100, Some(vec![0]), 31)]
#[case(Some(10), 100, None, 10)]
fn test_render_pdf(
    #[case] conf_w: Option<u32>,
    #[case] term_width: u32,
    #[case] page_indices: Option<Vec<u16>>,
    #[case] expected_width: u32,
) {
    let resize_mode = match conf_w {
        Some(w) => ResizeMode::Manual {
            width: Some(w),
            height: None,
        },
        None => ResizeMode::ClipTerminal,
    };
    let ctx = ctx_with(resize_mode, (term_width, 50), page_indices);

    let result = render_pdf(&ctx, PDF_DATA);
    assert!(result.is_ok(), "PDF generation failed");

    let img = result.unwrap();
    assert_eq!(img.width(), expected_width);

    let pixel = img.get_pixel(0, 0);
    assert_eq!(pixel, Rgba([255, 255, 255, 255]));
}

#[test]
fn test_render_pdf_invalid() {
    let pdf_data = br#"%PDF-1.4
invalid"#;
    let ctx = ctx_with(ResizeMode::ClipTerminal, (100, 50), None);

    let result = render_pdf(&ctx, pdf_data);
    assert!(result.is_err(), "PDF generation failed");
}

#[rstest]
#[case(vec![])]
#[case(vec![2])]
fn test_render_pdf_out_of_range(#[case] page_indices: Vec<u16>) {
    let ctx = ctx_with(ResizeMode::ClipTerminal, (100, 50), Some(page_indices));

    let result = render_pdf(&ctx, PDF_DATA);
    assert!(result.is_err(), "PDF generation failed");
}

#[rstest]
#[case(HTML_DATA)]
#[case(b"tests/fixtures/test.html")]
#[case(b"https://upload.wikimedia.org/wikipedia/commons/b/b9/Solid_red.png")]
fn test_render_html_chrome(#[case] html_data: &[u8]) {
    let _guard = CHROME_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let ctx = ctx_with(ResizeMode::Original, (100, 50), None);
    let result = render_html_chrome(&ctx, html_data);
    assert!(result.is_ok(), "HTML generation failed");

    let img = result.unwrap();

    // iterate through all pixels and check if any is red
    let mut red_found = false;
    for x in 0..img.width() {
        for y in 0..img.height() {
            let pixel = img.get_pixel(x, y);
            if pixel == Rgba([255, 0, 0, 255]) {
                red_found = true;
                break;
            }
        }
    }
    assert!(red_found, "Red pixel not found");
}

#[rstest]
#[case(RANDOM_DATA)] // non-utf-8
fn test_render_html_chrome_invalid(#[case] html_data: &[u8]) {
    let ctx = ctx_with(ResizeMode::Original, (100, 50), None);
    let result = render_html_chrome(&ctx, html_data);
    assert!(result.is_err(), "HTML generation should fail");
}

const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);
const TRANSPARENT: Rgba<u8> = Rgba([0, 0, 0, 0]);

#[rstest]
#[case(WHITE, TRANSPARENT, WHITE)]
#[case(BLACK, TRANSPARENT, BLACK)]
#[case(WHITE, BLACK, BLACK)]
#[case(WHITE, Rgba([255, 0, 0, 128]), Rgba([255, 127, 127, 255]))]
#[case(BLACK, Rgba([255, 0, 0, 128]), Rgba([128, 0, 0, 255]))]
fn test_add_background(
    #[case] color: Rgba<u8>,
    #[case] src_pixel: Rgba<u8>,
    #[case] expected_pixel: Rgba<u8>,
) {
    let mut img = DynamicImage::new_rgba8(1, 1); // 1x1 pixel
    img.as_mut_rgba8().unwrap().put_pixel(0, 0, src_pixel); // black, 100% alpha

    img = add_background(&img, &color);

    let pixel = img.get_pixel(0, 0);
    assert_eq!(
        pixel, expected_pixel,
        "Background color not applied correctly"
    );
}

#[test]
fn test_render_office_markdown_docx() {
    let ctx = ctx_with(ResizeMode::Original, (800, 400), None);
    let rendered = render_office_markdown(&ctx, DOCX_DATA, "docx").unwrap();
    let text = String::from_utf8(rendered).unwrap();
    assert!(text.contains("Hello World"));
    assert!(text.contains("This is a test document."));
}

#[test]
fn test_render_office_markdown_xlsx() {
    let ctx = ctx_with(ResizeMode::Original, (800, 400), None);
    let rendered = render_office_markdown(&ctx, XLSX_DATA, "xlsx").unwrap();
    let text = String::from_utf8(rendered).unwrap();
    assert!(text.contains('a') && text.contains('b') && text.contains('c'));
    assert!(text.contains('1') && text.contains('2') && text.contains('3'));
}

#[test]
fn test_render_office_markdown_unsupported_extension() {
    let ctx = ctx_with(ResizeMode::Original, (800, 400), None);
    let result = render_office_markdown(&ctx, RANDOM_DATA, "random");
    assert!(result.is_err());
}

#[test]
fn test_render_office_markdown_embeds_image() {
    let ctx = ctx_with(ResizeMode::Original, (800, 400), None);
    let rendered = render_office_markdown(&ctx, DOCX_WITH_IMAGE_DATA, "docx").unwrap();
    let text = String::from_utf8(rendered).unwrap();
    // an image embedded in the docx (not just a URL) still renders inline via Kitty, rather than
    // degrading to alt text.
    assert!(text.contains(KITTY_IMAGE_PREFIX));
}

#[test]
fn test_render_markdown() {
    let ctx = ctx_with(ResizeMode::Original, (800, 400), None);
    let base_dir = std::path::Path::new("tests/fixtures")
        .canonicalize()
        .unwrap();
    let base_dir = base_dir.as_path();
    let rendered = render_markdown(&ctx, MARKDOWN_DATA, base_dir).unwrap();
    let text = String::from_utf8(rendered).unwrap();
    assert!(text.contains("Title"));
    assert!(text.contains("bold"));
    // the relative image reference resolves against `base_dir` and renders inline via Kitty.
    assert!(text.contains(KITTY_IMAGE_PREFIX));
}

#[test]
fn test_render_markdown_page_selection() {
    let base_dir = std::path::Path::new("tests/fixtures")
        .canonicalize()
        .unwrap();
    let md = b"# Page One\n\nfirst content\n\n# Page Two\n\nsecond content\n";

    let ctx_all = ctx_with(ResizeMode::Original, (800, 400), None);
    let all = String::from_utf8(render_markdown(&ctx_all, md, &base_dir).unwrap()).unwrap();
    assert!(all.contains("Page One") && all.contains("second content"));

    // top-level headings split the document into pages, 0-indexed by request order.
    let ctx_page1 = ctx_with(ResizeMode::Original, (800, 400), Some(vec![0]));
    let page1 = String::from_utf8(render_markdown(&ctx_page1, md, &base_dir).unwrap()).unwrap();
    assert!(page1.contains("first content"));
    assert!(!page1.contains("second content"));

    let ctx_page2 = ctx_with(ResizeMode::Original, (800, 400), Some(vec![1]));
    let page2 = String::from_utf8(render_markdown(&ctx_page2, md, &base_dir).unwrap()).unwrap();
    assert!(!page2.contains("first content"));
    assert!(page2.contains("second content"));
}

#[test]
fn test_render_markdown_page_out_of_range() {
    let base_dir = std::path::Path::new("tests/fixtures")
        .canonicalize()
        .unwrap();
    let md = b"# Only Page\n\ncontent\n";
    let ctx = ctx_with(ResizeMode::Original, (800, 400), Some(vec![5]));
    let result = render_markdown(&ctx, md, &base_dir);
    assert!(result.is_err());
}

#[test]
fn test_render_office_markdown_page_selection() {
    // a single-sheet workbook is a single page: page 1 succeeds, page 2 is out of range.
    let ctx_page1 = ctx_with(ResizeMode::Original, (800, 400), Some(vec![0]));
    assert!(render_office_markdown(&ctx_page1, XLSX_DATA, "xlsx").is_ok());

    let ctx_page2 = ctx_with(ResizeMode::Original, (800, 400), Some(vec![1]));
    assert!(render_office_markdown(&ctx_page2, XLSX_DATA, "xlsx").is_err());
}
