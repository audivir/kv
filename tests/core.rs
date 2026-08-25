use image::Rgba;
use kv::*;
use rstest::rstest;
use std::path::PathBuf;

const PNG_DATA: &[u8] = include_bytes!("fixtures/test.png");
const SVG_DATA: &[u8] = include_bytes!("fixtures/test.svg");

fn default_ctx() -> KvContext {
    KvContext {
        input_type: InputType::Auto,
        resize_mode: ResizeMode::Original,
        term_size: (100, 50),
        page_indices: None,
        cache_mode: CacheMode::Disabled,
        background_color: None,
    }
}

#[rstest]
#[case("FF0000", Rgba([255, 0, 0, 255]))]
#[case("00FF00", Rgba([0, 255, 0, 255]))]
#[case("0000FF", Rgba([0, 0, 255, 255]))]
#[case("#FFFFFF", Rgba([255, 255, 255, 255]))]
#[case("#000000", Rgba([0, 0, 0, 255]))]
fn test_parse_color(#[case] color: &str, #[case] expected: Rgba<u8>) {
    let result = parse_color(color);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), expected);
}

#[rstest]
#[case("FF00")]
#[case("FF000000")]
#[case("#FF00GG")]
fn test_parse_color_invalid(#[case] color: &str) {
    let result = parse_color(color);
    assert!(result.is_err());
}

#[rstest]
#[case(100, 50, ResizeMode::Manual { width: Some(50), height: None }, 50, 25)] // explicit width
#[case(100, 50, ResizeMode::Manual { width: None, height: Some(25) }, 50, 25)] // explicit height
#[case(1000, 500, ResizeMode::ClipTerminal, 100, 50)] // auto-downscale
#[case(50, 50, ResizeMode::FitWidth, 100, 100)] // fullwidth
#[case(200, 25, ResizeMode::FitHeight, 400, 50)] // fullheight
#[case(500, 500, ResizeMode::FitTerminal, 50, 50)] // resize (bound by height)
#[case(1000, 200, ResizeMode::FitTerminal, 100, 20)] // resize (bound by width)
#[case(1000, 500, ResizeMode::Original, 1000, 500)] // noresize
fn test_calculate_dimensions(
    #[case] img_w: u32,
    #[case] img_h: u32,
    #[case] mode: ResizeMode,
    #[case] expected_w: u32,
    #[case] expected_h: u32,
) {
    let img_dims = (img_w, img_h);
    let term_size = (100, 50); // fixed

    let (w, h) = calculate_dimensions(img_dims, mode, term_size);

    assert_eq!(w, expected_w);
    assert_eq!(h, expected_h);
}

#[rstest]
#[case("1", vec![0])]
#[case("1,1", vec![0])]
#[case("1,2", vec![0, 1])]
#[case("2,1", vec![0, 1])]
#[case("2,,3", vec![1, 2])]
#[case("1-3", vec![0, 1, 2])]
#[case("1-3,5", vec![0, 1, 2, 4])]
#[case("1-3,5-7", vec![0, 1, 2, 4, 5, 6])]
fn test_parse_pages(#[case] input: &str, #[case] expected: Vec<u16>) {
    let result = parse_pages(input);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some(expected));
}

#[test]
fn test_parse_pages_empty() {
    let result = parse_pages("");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[rstest]
#[case("a")]
#[case("0")]
#[case("-1")]
#[case("1-2,4-3")]
fn test_parse_pages_invalid(#[case] input: &str) {
    let result = parse_pages(input);
    assert!(result.is_err());
}

#[rstest]
#[case(PathBuf::from("tests/fixtures/test.svg"), InputType::Svg)]
#[case(PathBuf::from("tests/fixtures/test.png"), InputType::Image)]
#[case(PathBuf::from("tests/fixtures/test.pdf"), InputType::Pdf)]
fn test_load_file(#[case] path: PathBuf, #[case] input_type: InputType) {
    let mut ctx = default_ctx();
    ctx.input_type = input_type;
    let result = load_file(&ctx, &path);
    assert!(result.is_ok());
    let result_auto = load_file(&ctx, &path);
    assert!(result_auto.is_ok());
}

#[rstest]
#[case(PathBuf::from("nonexistent"), InputType::Auto, "Failed to open file: nonexistent")]
#[case(
    PathBuf::from("tests/fixtures/test.random"),
    InputType::Auto,
    "Failed to decode input: Failed to decode image data"
)]
#[case(
    PathBuf::from("tests/fixtures/test.svg"),
    InputType::Image,
    "Failed to decode image data"
)]
fn test_load_file_invalid(
    #[case] path: PathBuf,
    #[case] input_type: InputType,
    #[case] err_msg: &str,
) {
    let mut ctx = default_ctx();
    ctx.input_type = input_type;
    let result = load_file(&ctx, &path);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), err_msg);
}

#[rstest]
#[case(
    PathBuf::from("tests/fixtures/test.png"),
    InputType::Svg,
    "Failed to parse SVG"
)]
#[case(
    PathBuf::from("tests/fixtures/test.pdf"),
    InputType::Svg,
    "Failed to parse SVG"
)]
fn test_load_file_invalid_svg(
    #[case] path: PathBuf,
    #[case] input_type: InputType,
    #[case] err_msg: &str,
) {
    let mut ctx = default_ctx();
    ctx.input_type = input_type;
    let result = load_file(&ctx, &path);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), err_msg);
}

#[rstest]
#[case("tests/fixtures/test.png".as_bytes())]
#[case(PNG_DATA)]
fn test_load_data(#[case] data: &[u8]) {
    let ctx = default_ctx();
    let result = load_data(&ctx, data, "");
    assert!(result.is_ok());
}

#[rstest]
#[case("tests/fixtures/test.svg".as_bytes())]
#[case(SVG_DATA)]
fn test_load_data_svg(#[case] data: &[u8]) {
    let ctx = default_ctx();
    let result = load_data(&ctx, data, "");
    assert!(result.is_ok());
}

#[rstest]
#[case(b"invalidbinary\x99\x98\x97\x96")]
fn test_load_data_invalid(#[case] data: &[u8]) {
    let ctx = default_ctx();
    let result = load_data(&ctx, data, "");
    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to decode input: Failed to decode image data"
    );
}

// non-image UTF-8 text that isn't a valid path falls back to plain-text data, not an error
#[rstest]
#[case("nonexistent".as_bytes())]
#[case(b"")]
fn test_load_data_text_fallback(#[case] data: &[u8]) {
    let ctx = default_ctx();
    let result = load_data(&ctx, data, "");
    assert!(matches!(result.unwrap(), LoadResult::Data(d) if d == data));
}

#[rstest]
#[case("<svg>invalid".as_bytes(), Some("Failed to parse SVG"))]
fn test_load_data_invalid_svg(#[case] data: &[u8], #[case] err_msg: Option<&str>) {
    let ctx = default_ctx();
    let result = load_data(&ctx, data, "");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), err_msg.unwrap());
}
