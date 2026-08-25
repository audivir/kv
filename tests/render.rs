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

fn ctx_with(resize_mode: ResizeMode, term_size: (u32, u32), page_indices: Option<Vec<u16>>) -> KvContext {
    KvContext {
        input_type: InputType::Auto,
        resize_mode,
        term_size,
        page_indices,
        cache_mode: CacheMode::Disabled,
        background_color: None,
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
