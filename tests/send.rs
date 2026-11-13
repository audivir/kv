use image::{DynamicImage, RgbaImage};
use kv::*;

/// 63 rows, 1008 px => 16 px cells.
const CELLS: Option<(u32, u32)> = Some((63, 1008));

fn send(height: u32, output: Option<String>, cells: Option<(u32, u32)>) -> String {
    let img = DynamicImage::ImageRgba8(RgbaImage::new(4, height));
    let mut out = Vec::new();
    send_image_with_cells(&mut out, img, output, Mode::Png, cells).unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

fn trailing_newlines(s: &str) -> usize {
    s.len() - s.trim_end_matches('\n').len()
}

#[test]
fn short_image_keeps_cursor_and_prints_row_newlines() {
    let out = send(40, None, CELLS); // 3 rows
    assert!(out.contains("a=T,f=100,C=1,m="));
    assert!(!out.contains("r="));
    assert_eq!(trailing_newlines(&out), 3);
}

#[test]
fn tall_image_pins_rows_and_prints_that_many_newlines() {
    let out = send(960, None, CELLS); // 60 rows
    assert!(out.contains("C=1,r=60,"));
    assert_eq!(trailing_newlines(&out), 60);
}

#[test]
fn image_taller_than_viewport_scrolls_past_it() {
    let out = send(2000, None, CELLS); // 125 rows > 63 rows of the pane
    assert!(out.contains("C=1,r=125,"));
    assert_eq!(trailing_newlines(&out), 125);
}

#[test]
fn row_count_rounds_up() {
    assert_eq!(trailing_newlines(&send(160, None, CELLS)), 10);
    assert_eq!(trailing_newlines(&send(161, None, CELLS)), 11);
}

#[test]
fn unknown_cell_size_falls_back_to_one_newline() {
    let out = send(960, None, None);
    assert!(!out.contains("C=1"));
    assert!(!out.contains("r="));
    assert_eq!(trailing_newlines(&out), 1);
}

#[test]
fn output_mode_writes_only_the_payload() {
    let out = send(960, Some("-".to_string()), CELLS);
    assert!(!out.contains("\x1b_G"));
    assert!(!out.ends_with('\n'));
}
