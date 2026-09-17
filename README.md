# kv

An image and document viewer for the Kitty Terminal Graphics Protocol.

**kv**, short for `Kitty Viewer`, is a spiritual successor to `tpix`, rewritten in Rust with:

- 16-bit PNG support,
- wider SVG support using `resvg`,
- PDF support using `pdfium`,
- HTML files and Office documents (docx, xlsx, pptx, ...) rendered as Markdown by default (using `htmd`/`anydoc`), or as an image via Chrome/`libreoffice`+`pdfium` with `--external` (cached per default for performance),
- Markdown rendering (headings, tables, lists, inline images, ...) using `pulldown-cmark-mdcat`, with images shown inline via the Kitty Graphics Protocol, including images embedded in raw HTML and remote images fetched over HTTP(S),
- Text output using `bat`.

## Installation

### Prerequisites

- For PDF support, download `libpdfium.dylib` or `libpdfium.so` from [pdfium](https://github.com/bblanchon/pdfium-binaries/releases) and copy it in the same directory as `kv`, one of the system library paths, or add the directory containing `libpdfium` library to `DYLD_LIBRARY_PATH` on macOS or `LD_LIBRARY_PATH` on Linux.
- Local HTML files and Office documents render as Markdown by default; no external tool is required. Images embedded in the source document (or referenced by a relative/HTTP(S) path) render inline via the Kitty Graphics Protocol.
- URLs passed directly (e.g. `kv https://example.org`) always render as a live screenshot via Chrome, regardless of `--external`; `headless_chrome` automatically downloads a Chrome binary on the first run.
- For `--external` (Office/HTML rendered as an image instead of Markdown), `soffice` (from `libreoffice`), `libpdfium`, and Chrome are required, depending on the input.
  > Caveats: Office files are cached per default for performance. Use `-C`/`--no-cache` to disable caching. If `soffice` is not found on `PATH`, `--external` falls back to Markdown rendering with a warning (Office documents only, not HTML).

### From Source

Ensure you have Rust installed.

```bash
git clone https://github.com/audivir/kv
cd kv
cargo build --release
cp target/release/kv ~/.local/bin/
```

## Usage

```bash
# view single image
kv image.png

# view multiple images
kv image1.png image2.jpg logo.svg

# pipe from stdin
cat photo.webp | kv

# resize to specific width
kv -w 500 image.png

# force full terminal width
kv -f image.png

# view specific pages of a pdf file
kv -P 1-3,34 pdf.pdf

# store a screenshot of an external domain as a png file
kv -o example.png https://example.org

# view an office document, rendered as markdown with inline images
kv document.docx

# render specific "pages" of an office document (sheets, slides, or heading-delimited sections)
kv -P 1-2 workbook.xlsx

# render an office document as an image instead, via an intermediate PDF
kv --external document.docx

# view a local html file, rendered as markdown with inline images (badges, ...)
kv page.html

# render a local html file as an image instead, via Chrome
kv --external page.html

# view a markdown file directly, with inline images
kv README.md
```

### Options

| Flag | Description |
| -------------------- | ------------------------------------------------------------------------------------- |
| `-w`, `--width` | Specify image width in pixels. |
| `-H`, `--height` | Specify image height in pixels. |
| `-f`, `--fullwidth` | Resize image to fill terminal width. |
| `-F`, `--fullheight` | Resize image to fill terminal height. |
| `-r`, `--resize` | Resize image to fill terminal. |
| `-n`, `--noresize` | Disable automatic resizing (show original size). |
| `-b`, `--background` | Add a background (useful for transparent images). |
| `-c`, `--color` | Set background color as hex string. Default: #FFFFFF. |
| `-m`, `--mode` | Set transmission mode (png, zlib, raw). Default: png. |
| `-o`, `--output` | Output to file as png, instead of kitty. |
| `-x`, `--overwrite` | Overwrite existing output file. |
| `-i`, `--input` | Set input type (auto, image, svg, pdf, html, office, markdown). Default: auto. |
| `-P`, `--pages` | Select pages to render (e.g. "1-3,34" or empty for all). For Markdown-rendered Office documents, a "page" is a top-level heading-delimited section (sheet or slide). Default: 1. |
| `-A`, `--all` | Select all pages. |
| `-l`, `--language` | Set language for syntax highlighting (e.g. "toml"). |
| `-N`, `--no-newline` | Do not add a newline after text data missing each input. (might mess up the terminal) |
| `-C`, `--no-cache` | Do not cache office files. |
| `--external` | Render Office/HTML as an image (soffice/Chrome) instead of Markdown. URLs always use Chrome. |
| `--theme` | Color scheme for Markdown code block syntax highlighting (light, dark). Default: dark. |
| `-p`, `--printname` | Print the filename before image. |
| `-t`, `--tty` | Force tty (ignore stdin check). |
| `-R`, `--remove` | Remove all images from terminal. |
| `--plugins` | Print the plugins configuration file path (will be created if it doesn't exist). |

## Plugins

You can extend `kv` to support additional file formats by adding external converters to the configuration file. To find or edit your configuration, run:

```bash
# Open the configuration file in your preferred editor
nano $(kv --plugins)
```

For a detailed explanation of the configuration file format, see the header of the configuration file.

### Example: Ghostscript for EPS Support

To render EPS files using `ghostscript`, add the following to your plugin configuration file:

```toml
[eps-converter]
extensions = ["eps"]
output = "image"
path = "gs -q -dSAFER -dBATCH -dNOPAUSE -sDEVICE=pngalpha -r300 -dEPSCrop -sOutputFile=- -"
```

## Roadmap

Planned, not yet implemented:

- Fallback rendering (Sixel, iTerm2, Unicode blocks) for terminals without the Kitty graphics protocol,
- animated GIF and video playback,
- directory/gallery browsing.

## License

MIT License. See [LICENSE](LICENSE) for details.

## Acknowledgments

- Based on the logic of [tpix](https://github.com/jesvedberg/tpix) by Jesper Svedberg (MIT License).
- Uses [resvg](https://github.com/RazrFalcon/resvg) for SVG rendering (MIT License).
- Uses [bat](https://github.com/sharkdp/bat) for text rendering (MIT License).
- Uses pre-compiled [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases) for PDF rendering (MIT License).
- "fixtures/semi_transparent.png" is by Nguyễn Trí Minh Hoàng and is licensed under CC BY-SA 3.0.
