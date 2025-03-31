use kurbo::{Point, Size};
use skia_safe::textlayout::FontCollection;
use skia_safe::FontMgr;
use std::borrow::Cow;
use std::cell::OnceCell;
use std::fmt;

pub use selection::Selection;
pub use style::{FontStretch, FontStyle, FontWeight, StyleProperty, TextStyle};
pub use text_run::TextRun;

use crate::drawing::{RectWithBaseline, ToSkia};

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

/// Represents formatted text that is shaped, laid out, and ready to be painted.
pub struct TextLayout {
    pub inner: skia_safe::textlayout::Paragraph,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self::new(&TextStyle::default(), &[])
    }
}

impl TextLayout {
    /// Constructs a new text layout from a default text style and attributed text runs.
    pub fn new(style: &TextStyle, text: &[TextRun]) -> TextLayout {
        let font_collection = get_font_collection();
        let mut paragraph_style = skia_safe::textlayout::ParagraphStyle::new();
        paragraph_style.set_text_style(&style.to_skia());
        paragraph_style.set_apply_rounding_hack(false);
        let mut builder = skia_safe::textlayout::ParagraphBuilder::new(&paragraph_style, font_collection);

        for run in text.into_iter() {
            let mut style = style.clone();
            for prop in run.styles {
                prop.apply(&mut style);
            }
            builder.push_style(&style.to_skia());
            builder.add_text(&run.str);
            builder.pop();
        }

        Self { inner: builder.build() }
    }

    /// Constructs a new text layout from a default text style and a string.
    pub fn from_str(style: &TextStyle, text: &str) -> TextLayout {
        let run = TextRun { str: text, styles: &[] };
        Self::new(style, &[run])
    }

    /// Recomputes the layout of the text given the specified available width.
    pub fn layout(&mut self, width: f64) {
        self.inner.layout(width as f32);
    }

    /// Paints the text at the specified position on a skia canvas.
    ///
    /// The position is the position of the upper-left corner of the (ascender) text box.
    pub fn paint(&self, canvas: &skia_safe::Canvas, pos: Point) {
        self.inner.paint(canvas, pos.to_skia());
    }

    /// Returns the height of the text layout.
    pub fn height(&self) -> f64 {
        self.inner.height() as f64
    }

    /// Returns the size of the text layout.
    pub fn size(&self) -> Size {
        Size {
            width: self.inner.longest_line() as f64,
            height: self.inner.height() as f64,
        }
    }

    /// Returns the baseline of the first line of text.
    pub fn baseline(&self) -> f64 {
        self.inner.alphabetic_baseline() as f64
    }

    pub fn rect_with_baseline(&self) -> RectWithBaseline {
        RectWithBaseline {
            rect: self.size().to_rect(),
            baseline: self.baseline(),
        }
    }
}

impl<'a, const N: usize> From<&'a [TextRun<'a>; N]> for TextLayout {
    fn from(runs: &'a [TextRun; N]) -> Self {
        TextLayout::new(&TextStyle::default(), runs)
    }
}

// `text!` macro support
#[doc(hidden)]
pub fn cow_format_args(args: fmt::Arguments) -> Cow<str> {
    match args.as_str() {
        Some(s) => Cow::Borrowed(s),
        None => Cow::Owned(args.to_string()),
    }
}
