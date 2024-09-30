use std::borrow::Cow;
use std::cell::OnceCell;
use std::fmt;

use skia_safe::textlayout::FontCollection;
use skia_safe::FontMgr;

pub use selection::Selection;
pub use style::{FontStretch, FontStyle, FontWeight, TextStyle};
pub use text_run::TextRun;

use crate::drawing::{FromSkia, ToSkia};

mod selection;
mod skia;
mod style;
mod text_run;

thread_local! {
    static FONT_COLLECTION: OnceCell<FontCollection> = OnceCell::new();
}

/// Returns the FontCollection for the current thread.
///
/// FontCollections (and other objects that reference them, e.g. Paragraph)
/// are bound to the thread in which they were created.
pub(crate) fn get_font_collection() -> FontCollection {
    // Ideally I'd like to have only one font collection for all threads.
    // However, FontCollection isn't Send or Sync, and `Paragraphs` hold a reference to a FontCollection,
    // so, to be able to create Paragraphs from different threads, there must be one FontCollection
    // per thread.
    //
    // See also https://github.com/rust-skia/rust-skia/issues/537
    FONT_COLLECTION.with(|fc| {
        fc.get_or_init(|| {
            let mut font_collection = FontCollection::new();
            font_collection.set_default_font_manager(FontMgr::new(), None);
            font_collection
        })
            .clone()
    })
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextAlign {
    Start,
    End,
    Middle,
    Justify,
}

pub struct TextLayout {
    pub inner: skia_safe::textlayout::Paragraph,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self::new(&[])
    }
}

impl TextLayout {
    /// Constructs a new text layout from attributed text runs.
    pub fn new(text: &[TextRun]) -> TextLayout {
        let font_collection = get_font_collection();
        let mut text_style = skia_safe::textlayout::TextStyle::new();
        text_style.set_font_size(16.0 as f32); // TODO default font size
        let mut paragraph_style = skia_safe::textlayout::ParagraphStyle::new();
        paragraph_style.set_text_style(&text_style);
        let mut builder = skia_safe::textlayout::ParagraphBuilder::new(&paragraph_style, font_collection);

        for run in text.into_iter() {
            let style = run.style.to_skia();
            builder.push_style(&style);
            builder.add_text(&run.str);
            builder.pop();
        }

        Self { inner: builder.build() }
    }
}

/*
/// Lines of formatted (shaped and layouted) text.
pub struct FormattedText {
    pub inner: skia_safe::textlayout::Paragraph,
}

impl Default for FormattedText {
    fn default() -> Self {
        let paragraph_style = sk::textlayout::ParagraphStyle::new();
        let font_collection = get_font_collection();
        FormattedText {
            inner: sk::textlayout::ParagraphBuilder::new(&paragraph_style, font_collection).build(),
        }
    }
}

impl FormattedText {
    /// Creates a new formatted text object for the specified text runs (text + associated style).

    // Q: take a slice of AttributedRange? No, because the slice needs to be constructed, maybe unnecessarily.
    // With IntoIterator this works with everything (there are no slices involved)

    pub fn new<'a>(text: impl IntoIterator<Item=AttributedRange<'a>>) -> Self {
        let font_collection = get_font_collection();
        let mut text_style = sk::textlayout::TextStyle::new();
        text_style.set_font_size(16.0 as sk::scalar); // TODO default font size
        let mut paragraph_style = sk::textlayout::ParagraphStyle::new();
        paragraph_style.set_text_style(&text_style);
        let mut builder = sk::textlayout::ParagraphBuilder::new(&paragraph_style, font_collection);

        for run in text.into_iter() {
            let style = run.style.to_skia();
            builder.push_style(&style);
            builder.add_text(&run.str);
            builder.pop();
        }

        Self { inner: builder.build() }
    }

    pub fn from_attributed_str(text: &AttributedStr) -> Self {
        Self::new(text.iter().cloned())
    }

    /// Layouts or relayouts the text under the given width constraint.
    pub fn layout(&mut self, available_width: f64) {
        self.inner.layout(available_width as f32);
    }

    /// Returns bounding rectangles for the specified range of text, specified in byte offsets.
    pub fn get_rects_for_range(&self, range: Range<usize>) -> Vec<Rect> {
        let text_boxes = self.inner.get_rects_for_range(range, RectHeightStyle::Tight, RectWidthStyle::Tight);
        text_boxes.iter().map(|r| Rect::from_skia(r.rect)).collect()
    }
}
*/

// `text!` macro support
#[doc(hidden)]
pub fn cow_format_args(args: fmt::Arguments) -> Cow<str> {
    match args.as_str() {
        Some(s) => Cow::Borrowed(s),
        None => Cow::Owned(args.to_string()),
    }
}
