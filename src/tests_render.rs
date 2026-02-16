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
