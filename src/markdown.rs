use anydoc::model::{AssetId, Block, CellSlot, Document, ImageSource, Inline, LinkTarget, Style};
use anyhow::{Context, Result};
use pulldown_cmark::{
    Alignment, CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};
use pulldown_cmark_mdcat::resources::{
    DispatchingResourceHandler, FileResourceHandler, MimeData, ResourceUrlHandler, filter_schemes,
};
use pulldown_cmark_mdcat::terminal::TerminalSize;
use pulldown_cmark_mdcat::terminal::capabilities::kitty::KittyGraphicsProtocol;
use pulldown_cmark_mdcat::terminal::capabilities::{
    ImageCapability, StyleCapability, TerminalCapabilities,
};
use pulldown_cmark_mdcat::{Environment, Settings, Theme};
use std::collections::HashMap;
use std::io::Error;
use std::path::{Path, PathBuf};

use crate::KvContext;

/// Bytes read from a local `file:` resource, kept small enough that a malformed or malicious
/// asset cannot exhaust memory while rendering.
const RESOURCE_READ_LIMIT: u64 = 128 * 1024 * 1024;

/// Fetcher for `http(s)` resources referenced from Markdown or HTML (badges, remote images), so
/// they render inline like they would on GitHub instead of degrading to a plain link.
struct HttpResourceHandler;

impl ResourceUrlHandler for HttpResourceHandler {
    fn read_resource(&self, url: &url::Url) -> std::io::Result<MimeData> {
        let url = filter_schemes(&["http", "https"], url)?;
        let mut response = ureq::get(url.as_str())
            .call()
            .map_err(Error::other)?;
        let mime_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<mime::Mime>().ok());
        let data = response
            .body_mut()
            .read_to_vec()
            .map_err(Error::other)?;
        Ok(MimeData { mime_type, data })
    }
}

fn resource_handler() -> DispatchingResourceHandler {
    DispatchingResourceHandler::new(vec![
        Box::new(FileResourceHandler::new(RESOURCE_READ_LIMIT)),
        Box::new(HttpResourceHandler),
    ])
}

fn terminal_capabilities() -> TerminalCapabilities {
    TerminalCapabilities {
        style: Some(StyleCapability::Ansi),
        image: Some(ImageCapability::Kitty(KittyGraphicsProtocol)),
        marks: None,
    }
}

/// Splits a flat event stream into "pages" at top-level headings of the shallowest level used
/// (e.g. sheet/slide titles), then keeps only the pages named by `page_indices`. Documents
/// without any top-level heading are a single page. `None` keeps every event, unfiltered.
fn select_pages<'e>(
    events: Vec<Event<'e>>,
    page_indices: Option<&[u16]>,
) -> Result<Vec<Event<'e>>> {
    let Some(page_indices) = page_indices else {
        return Ok(events);
    };

    let mut depth = 0i32;
    let mut top_level_headings = Vec::new();
    for (i, event) in events.iter().enumerate() {
        match event {
            Event::Start(tag) => {
                if depth == 0
                    && let Tag::Heading { level, .. } = tag
                {
                    top_level_headings.push((i, *level));
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            _ => {}
        }
    }

    let mut page_starts = match top_level_headings.iter().map(|&(_, level)| level).min() {
        None => vec![0],
        Some(min_level) => top_level_headings
            .iter()
            .filter(|&&(_, level)| level == min_level)
            .map(|&(i, _)| i)
            .collect(),
    };
    if page_starts.first() != Some(&0) {
        page_starts.insert(0, 0);
    }

    let page_ranges: Vec<std::ops::Range<usize>> = page_starts
        .iter()
        .enumerate()
        .map(|(i, &start)| start..page_starts.get(i + 1).copied().unwrap_or(events.len()))
        .filter(|range| !range.is_empty())
        .collect();

    if let Some(&max_index) = page_indices.iter().max()
        && usize::from(max_index) >= page_ranges.len()
    {
        anyhow::bail!("Page index out of range (must be <= {})", page_ranges.len());
    }

    let mut keep = vec![false; events.len()];
    for &index in page_indices {
        for flag in &mut keep[page_ranges[usize::from(index)].clone()] {
            *flag = true;
        }
    }

    Ok(events
        .into_iter()
        .zip(keep)
        .filter_map(|(event, keep)| keep.then_some(event))
        .collect())
}

fn render_events<'e>(ctx: &KvContext, base_dir: &Path, events: Vec<Event<'e>>) -> Result<Vec<u8>> {
    let events = select_pages(events, ctx.page_indices.as_deref())?;
    // languages `bat`'s bundled syntax set does not cover fall back to unhighlighted plain text.
    let assets = bat::assets::HighlightingAssets::from_binary();
    let syntax_set = assets.get_syntax_set().context("Failed to load syntax definitions")?;
    let settings = Settings {
        terminal_capabilities: terminal_capabilities(),
        // the image-fitting logic needs real pixel/cell dimensions to scale embedded images
        // down to the terminal; `TerminalSize::detect` queries those from the real terminal.
        terminal_size: TerminalSize::detect().unwrap_or_default(),
        syntax_set,
        theme: Theme::default(),
        syntax_theme: Some(assets.get_theme(ctx.color_scheme.theme_name()).clone()),
    };
    let environment =
        Environment::for_local_directory(&base_dir).context("Failed to resolve base directory")?;
    let resource_handler = resource_handler();

    let mut output = Vec::new();
    pulldown_cmark_mdcat::push_tty(
        &settings,
        &environment,
        &resource_handler,
        &mut output,
        events.into_iter(),
    )
    .context("Failed to render Markdown")?;
    Ok(output)
}

const MARKDOWN_OPTIONS: Options =
    Options::ENABLE_TABLES.union(Options::ENABLE_STRIKETHROUGH).union(Options::ENABLE_TASKLISTS);

/// Renders Markdown to terminal-ready bytes, resolving relative image references against
/// `base_dir`.
pub fn render_markdown(ctx: &KvContext, data: &[u8], base_dir: &Path) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(data).context("Markdown input is not valid UTF-8")?;
    let events: Vec<Event> = Parser::new_ext(text, MARKDOWN_OPTIONS).collect();
    let events = normalize_embedded_html(events)?;
    render_events(ctx, base_dir, events)
}

/// Converts a standalone HTML document to Markdown via `htmd` and renders it like
/// [`render_markdown`], resolving relative image references against `base_dir`.
pub fn render_html_markdown(ctx: &KvContext, data: &[u8], base_dir: &Path) -> Result<Vec<u8>> {
    let html = std::str::from_utf8(data).context("HTML input is not valid UTF-8")?;
    let markdown = htmd::convert(html).context("Failed to convert HTML to Markdown")?;
    let events: Vec<Event> = Parser::new_ext(&markdown, MARKDOWN_OPTIONS).collect();
    render_events(ctx, base_dir, events)
}

/// Normalizes raw HTML embedded in Markdown (badges, centered images, `<details>`, ...), which
/// has no equivalent `pulldown_cmark::Event` and would otherwise print as literal source text.
/// Renders the document to HTML the same way GitHub does (merging CommonMark with the embedded
/// HTML), converts that back to plain Markdown via `htmd`, and reparses it, so e.g. `<img>` and
/// `<a>` tags become real image and link events. Documents without embedded HTML keep their
/// original events unchanged, so plain CommonMark is unaffected by the round trip.
fn normalize_embedded_html(events: Vec<Event<'_>>) -> Result<Vec<Event<'static>>> {
    if !events.iter().any(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_))) {
        return Ok(events.into_iter().map(Event::into_static).collect());
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    let markdown = htmd::convert(&html).context("Failed to normalize embedded HTML")?;
    Ok(Parser::new_ext(&markdown, MARKDOWN_OPTIONS).map(Event::into_static).collect())
}

/// Converts an Office document to Markdown via `anydoc` and renders it like [`render_markdown`],
/// writing embedded images to a temporary directory so they render inline instead of degrading
/// to alt text.
pub fn render_office_markdown(ctx: &KvContext, data: &[u8], extension: &str) -> Result<Vec<u8>> {
    let format = anydoc::Format::from_extension(extension);
    let document = anydoc::to_document(data, format)
        .context("Failed to convert office document to Markdown")?;

    let asset_dir = tempfile::tempdir().context("Failed to create temporary asset directory")?;
    let events = document_to_events(&document, asset_dir.path())?;
    render_events(ctx, asset_dir.path(), events)
}

/// Cache of embedded [`Document`] assets, written to disk on first reference so the terminal
/// renderer can read each one back through a `file:` URL.
struct AssetWriter<'a> {
    document: &'a Document,
    dir: &'a Path,
    written: HashMap<usize, PathBuf>,
}

impl<'a> AssetWriter<'a> {
    fn path_for(&mut self, id: AssetId) -> Result<PathBuf> {
        if let Some(path) = self.written.get(&id.0) {
            return Ok(path.clone());
        }
        let asset = self
            .document
            .assets
            .get(id.0)
            .context("Document references an asset id outside its own asset list")?;
        let path = self.dir.join(format!(
            "asset-{}{}",
            id.0,
            extension_for(&asset.media_type)
        ));
        std::fs::write(&path, &asset.bytes)
            .with_context(|| format!("Failed to write embedded asset to {}", path.display()))?;
        self.written.insert(id.0, path.clone());
        Ok(path)
    }
}

fn extension_for(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/bmp" => ".bmp",
        "image/svg+xml" => ".svg",
        "image/webp" => ".webp",
        "image/tiff" => ".tiff",
        _ => ".bin",
    }
}

fn heading_level(level: u8) -> HeadingLevel {
    match level.clamp(1, 6) {
        1 => HeadingLevel::H1,
        2 => HeadingLevel::H2,
        3 => HeadingLevel::H3,
        4 => HeadingLevel::H4,
        5 => HeadingLevel::H5,
        _ => HeadingLevel::H6,
    }
}

fn document_to_events(document: &Document, asset_dir: &Path) -> Result<Vec<Event<'static>>> {
    let mut events = Vec::new();
    let mut assets = AssetWriter {
        document,
        dir: asset_dir,
        written: HashMap::new(),
    };

    for block in &document.blocks {
        push_block(block, &mut events, &mut assets)?;
    }

    if !document.notes.is_empty() {
        events.push(Event::Start(heading_tag(2)));
        events.push(Event::Text(CowStr::Borrowed("Notes")));
        events.push(Event::End(TagEnd::Heading(HeadingLevel::H2)));
        for note in &document.notes {
            events.push(Event::Start(Tag::Paragraph));
            events.push(Event::Start(Tag::Strong));
            events.push(Event::Text(CowStr::from(format!("[{}]", note.id))));
            events.push(Event::End(TagEnd::Strong));
            events.push(Event::End(TagEnd::Paragraph));
            for block in &note.blocks {
                push_block(block, &mut events, &mut assets)?;
            }
        }
    }

    Ok(events)
}

fn heading_tag(level: u8) -> Tag<'static> {
    Tag::Heading {
        level: heading_level(level),
        id: None,
        classes: vec![],
        attrs: vec![],
    }
}

fn push_block(
    block: &Block,
    events: &mut Vec<Event<'static>>,
    assets: &mut AssetWriter,
) -> Result<()> {
    match block {
        Block::Heading { level, content, .. } => {
            events.push(Event::Start(heading_tag(*level)));
            push_inlines(content, events, assets)?;
            events.push(Event::End(TagEnd::Heading(heading_level(*level))));
        }
        Block::Paragraph(inlines) => {
            events.push(Event::Start(Tag::Paragraph));
            push_inlines(inlines, events, assets)?;
            events.push(Event::End(TagEnd::Paragraph));
        }
        Block::List(list) => {
            let start = list.ordered().then_some(list.start);
            events.push(Event::Start(Tag::List(start)));
            for item in &list.items {
                events.push(Event::Start(Tag::Item));
                for item_block in &item.blocks {
                    push_block(item_block, events, assets)?;
                }
                events.push(Event::End(TagEnd::Item));
            }
            events.push(Event::End(TagEnd::List(list.ordered())));
        }
        Block::Table(table) => {
            let column_count = table.grid.iter().map(Vec::len).max().unwrap_or(0);
            events.push(Event::Start(Tag::Table(vec![
                Alignment::None;
                column_count
            ])));

            let header_rows = table.header_rows.min(table.grid.len());
            if header_rows > 0 {
                events.push(Event::Start(Tag::TableHead));
                push_table_row(&table.grid[0], events, assets)?;
                events.push(Event::End(TagEnd::TableHead));
            }
            for row in table.grid.iter().skip(header_rows) {
                events.push(Event::Start(Tag::TableRow));
                push_table_row(row, events, assets)?;
                events.push(Event::End(TagEnd::TableRow));
            }
            events.push(Event::End(TagEnd::Table));
        }
        Block::BlockQuote(blocks) => {
            events.push(Event::Start(Tag::BlockQuote(None)));
            for inner in blocks {
                push_block(inner, events, assets)?;
            }
            events.push(Event::End(TagEnd::BlockQuote(None)));
        }
        Block::CodeBlock { lang, text } => {
            let kind = match lang {
                Some(lang) if !lang.is_empty() => CodeBlockKind::Fenced(CowStr::from(lang.clone())),
                _ => CodeBlockKind::Indented,
            };
            events.push(Event::Start(Tag::CodeBlock(kind)));
            events.push(Event::Text(CowStr::from(text.clone())));
            events.push(Event::End(TagEnd::CodeBlock));
        }
        Block::Rule => events.push(Event::Rule),
        Block::Math(tex) => {
            events.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(
                CowStr::Borrowed("math"),
            ))));
            events.push(Event::Text(CowStr::from(tex.clone())));
            events.push(Event::End(TagEnd::CodeBlock));
        }
    }
    Ok(())
}

/// Pushes one table row as `TableCell` events. Cells hold inline content directly (no
/// `Paragraph` wrapper), unlike other blocks.
fn push_table_row(
    row: &[CellSlot],
    events: &mut Vec<Event<'static>>,
    assets: &mut AssetWriter,
) -> Result<()> {
    for slot in row {
        events.push(Event::Start(Tag::TableCell));
        if let CellSlot::Origin(cell) = slot {
            for block in &cell.blocks {
                if let Block::Paragraph(inlines) = block {
                    push_inlines(inlines, events, assets)?;
                }
            }
        }
        events.push(Event::End(TagEnd::TableCell));
    }
    Ok(())
}

fn push_inlines(
    inlines: &[Inline],
    events: &mut Vec<Event<'static>>,
    assets: &mut AssetWriter,
) -> Result<()> {
    for inline in inlines {
        push_inline(inline, events, assets)?;
    }
    Ok(())
}

fn push_inline(
    inline: &Inline,
    events: &mut Vec<Event<'static>>,
    assets: &mut AssetWriter,
) -> Result<()> {
    match inline {
        Inline::Text { text, style } => push_styled_text(text, *style, events),
        Inline::Link { content, target } => {
            events.push(Event::Start(Tag::Link {
                link_type: LinkType::Inline,
                dest_url: CowStr::from(link_destination(target)),
                title: CowStr::Borrowed(""),
                id: CowStr::Borrowed(""),
            }));
            push_inlines(content, events, assets)?;
            events.push(Event::End(TagEnd::Link));
        }
        Inline::Image { alt, source } => push_image(alt, source, events, assets)?,
        Inline::Anchor(_) => {}
        Inline::NoteRef(id) => events.push(Event::Text(CowStr::from(format!("[{}]", id)))),
        Inline::LineBreak => events.push(Event::HardBreak),
        Inline::Math(tex) => events.push(Event::Code(CowStr::from(tex.clone()))),
        Inline::Checkbox(checked) => events.push(Event::TaskListMarker(*checked)),
    }
    Ok(())
}

fn push_styled_text(text: &str, style: Style, events: &mut Vec<Event<'static>>) {
    if text.is_empty() {
        return;
    }
    if style.code {
        events.push(Event::Code(CowStr::from(text.to_string())));
        return;
    }

    let mut ends = Vec::new();
    if style.strike {
        events.push(Event::Start(Tag::Strikethrough));
        ends.push(TagEnd::Strikethrough);
    }
    if style.bold {
        events.push(Event::Start(Tag::Strong));
        ends.push(TagEnd::Strong);
    }
    if style.italic {
        events.push(Event::Start(Tag::Emphasis));
        ends.push(TagEnd::Emphasis);
    }

    events.push(Event::Text(CowStr::from(text.to_string())));

    for end in ends.into_iter().rev() {
        events.push(Event::End(end));
    }
}

fn link_destination(target: &LinkTarget) -> String {
    match target {
        LinkTarget::External(url) | LinkTarget::Relative(url) => url.clone(),
        LinkTarget::Anchor(id) => format!("#{}", id),
    }
}

fn push_image(
    alt: &str,
    source: &ImageSource,
    events: &mut Vec<Event<'static>>,
    assets: &mut AssetWriter,
) -> Result<()> {
    let dest_url = match source {
        ImageSource::External(url) => Some(url.clone()),
        ImageSource::Asset(id) => {
            let path = assets.path_for(*id)?;
            Some(
                url::Url::from_file_path(&path)
                    .map_err(|()| {
                        anyhow::anyhow!("Asset path is not absolute: {}", path.display())
                    })?
                    .to_string(),
            )
        }
        ImageSource::Unavailable => None,
    };

    match dest_url {
        Some(dest_url) => {
            events.push(Event::Start(Tag::Image {
                link_type: LinkType::Inline,
                dest_url: CowStr::from(dest_url),
                title: CowStr::Borrowed(""),
                id: CowStr::Borrowed(""),
            }));
            if !alt.is_empty() {
                events.push(Event::Text(CowStr::from(alt.to_string())));
            }
            events.push(Event::End(TagEnd::Image));
        }
        None if !alt.is_empty() => events.push(Event::Text(CowStr::from(alt.to_string()))),
        None => {}
    }
    Ok(())
}
