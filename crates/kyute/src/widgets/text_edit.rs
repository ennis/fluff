use crate::application::{spawn, wait_for};
use crate::drawing::{FromSkia, Paint, ToSkia};
use crate::element::{Element, ElementMethods};
use crate::event::Event;
use crate::handler::Handler;
use crate::layout::{BoxConstraints, Geometry};
use crate::text::{get_font_collection, Selection, TextAlign, TextLayout, TextStyle};
use crate::{Color, PaintCtx};
use keyboard_types::Key;
use kurbo::{Point, Rect, Size, Vec2};
use skia_safe::textlayout::{RectHeightStyle, RectWidthStyle};
use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;
use std::time::Duration;
use taffy::{AvailableSpace, LayoutInput, LayoutOutput};
use unicode_segmentation::GraphemeCursor;

#[derive(Debug, Copy, Clone)]
pub enum Movement {
    Left,
    Right,
    LeftWord,
    RightWord,
}

fn prev_grapheme_cluster(text: &str, offset: usize) -> Option<usize> {
    let mut c = GraphemeCursor::new(offset, text.len(), true);
    c.prev_boundary(text, 0).unwrap()
}

fn next_grapheme_cluster(text: &str, offset: usize) -> Option<usize> {
    let mut c = GraphemeCursor::new(offset, text.len(), true);
    c.next_boundary(text, 0).unwrap()
}

/// If `other` comes before `self`, the cursor is placed at the beginning of the selection.
fn add_selections(this: Selection, other: Selection) -> Selection {
    let min = this.min().min(other.min());
    let max = this.max().max(other.max());
    if other.min() < this.min() {
        Selection { start: max, end: min }
    } else {
        Selection { start: min, end: max }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum WrapMode {
    Wrap,
    NoWrap,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TextOverflow {
    Ellipsis,
    Clip,
}

struct TextEditState {
    text: String,
    selection: Selection,
    text_style: TextStyle<'static>,
    last_width_constraint: f32,
    paragraph: skia_safe::textlayout::Paragraph,
    selection_color: Color,
    caret_color: Color,
    relayout: bool,
    scroll_offset: Vec2,
    wrap_mode: WrapMode,
    text_overflow: TextOverflow,
    align: TextAlign,
    line_clamp: Option<usize>,
    size: Size,
}

impl TextEditState {
    fn rebuild_paragraph(&mut self) {
        let font_collection = get_font_collection();
        let mut text_style = skia_safe::textlayout::TextStyle::new();
        text_style.set_font_size(16.0); // TODO default font size
        let mut paragraph_style = skia_safe::textlayout::ParagraphStyle::new();
        paragraph_style.set_text_style(&text_style);
        paragraph_style.set_apply_rounding_hack(false);

        if let Some(line_clamp) = self.line_clamp {
            paragraph_style.set_max_lines(line_clamp);
        }

        match self.text_overflow {
            TextOverflow::Ellipsis => {
                paragraph_style.set_ellipsis("…");
            }
            TextOverflow::Clip => {}
        }

        paragraph_style.set_text_align(self.align.to_skia());

        let mut builder = skia_safe::textlayout::ParagraphBuilder::new(&paragraph_style, font_collection);
        let style = self.text_style.to_skia();
        builder.push_style(&style);
        builder.add_text(&self.text);
        builder.pop();

        self.paragraph = builder.build();
    }

    fn set_selection(&mut self, selection: Selection) -> bool {
        if selection != self.selection {
            self.selection = selection;
            self.scroll_in_view(self.selection.end);

            // debug the current cursor position by printing the string with <HERE> at the cursor position
            let mut text = self.text.clone();
            text.insert_str(self.selection.end, "<HERE>");
            eprintln!("cursor changed position: {text}");

            true
        } else {
            false
        }
    }

    fn set_cursor_at_text_position(&mut self, pos: usize, keep_anchor: bool) -> bool {
        self.set_selection(if keep_anchor {
            Selection {
                start: self.selection.start,
                end: pos,
            }
        } else {
            Selection::empty(pos)
        })
    }

    fn move_cursor_to_next_word(&mut self, keep_anchor: bool) -> bool {
        let end = next_word_boundary(&self.text, self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    fn move_cursor_to_prev_word(&mut self, keep_anchor: bool) -> bool {
        let end = prev_word_boundary(&self.text, self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    fn move_cursor_to_next_grapheme(&mut self, keep_anchor: bool) -> bool {
        let end = next_grapheme_cluster(&self.text, self.selection.end).unwrap_or(self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    fn move_cursor_to_prev_grapheme(&mut self, keep_anchor: bool) -> bool {
        let end = prev_grapheme_cluster(&self.text, self.selection.end).unwrap_or(self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    /// Scrolls the text to make the given text position visible.
    fn scroll_in_view(&mut self, text_offset: usize) -> bool {
        let rects = self.paragraph.get_rects_for_range(
            text_offset..text_offset + 1,
            RectHeightStyle::Tight,
            RectWidthStyle::Tight,
        );
        if rects.is_empty() {
            return false;
        }
        let rect = Rect::from_skia(rects[0].rect);
        let scroll_offset = self.scroll_offset.x;

        let scroll_offset = if rect.x1 > self.size.width + scroll_offset {
            rect.x1 - self.size.width
        } else if rect.x0 < scroll_offset {
            rect.x0
        } else {
            scroll_offset
        };
        if scroll_offset != self.scroll_offset.x {
            self.scroll_offset.x = scroll_offset;
            true
        } else {
            false
        }
    }

    fn text_position_for_point(&self, point: Point) -> usize {
        let point = point + self.scroll_offset;
        // NOTE: get_glyph_position_at_coordinate returns a text position in bytes, not a glyph
        // position, as the name suggests.
        self.paragraph
            .get_glyph_position_at_coordinate(point.to_skia())
            .position as usize
    }

    /// NOTE: valid only after first layout.
    fn set_cursor_at_point(&mut self, point: Point, keep_anchor: bool) -> bool {
        let point = point + self.scroll_offset;
        let pos = self.paragraph.get_glyph_position_at_coordinate(point.to_skia());
        self.set_cursor_at_text_position(pos.position as usize, keep_anchor)
    }

    fn select_word_under_cursor(&mut self) -> bool {
        let selection = self.selection;
        let range = self.paragraph.get_word_boundary(selection.end as u32);
        self.set_selection(Selection {
            start: range.start,
            end: range.end,
        })
    }

    fn word_selection_at_text_position(&self, pos: usize) -> Selection {
        let range = self.paragraph.get_word_boundary(pos as u32);
        let word = Selection {
            start: range.start,
            end: range.end,
        };
        // skia reports a word boundary for newlines, ignore it
        if self.text[range.clone()].starts_with('\n') {
            return Selection::empty(pos);
        }
        word
    }

    fn select_line_under_cursor(&mut self) -> bool {
        let text = &self.text;
        let selection = self.selection;
        let start = text[..selection.end].rfind('\n').map_or(0, |i| i + 1);
        let end = text[selection.end..]
            .find('\n')
            .map_or(text.len(), |i| selection.end + i);
        self.set_selection(Selection { start, end })
    }
}

const CARET_BLINK_INITIAL_DELAY: Duration = Duration::from_secs(1);
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);

fn next_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = offset;
    enum State {
        LeadingWhitespace,
        Alnum,
        NotAlnum,
    }
    let mut state = State::LeadingWhitespace;
    for ch in text[offset..].chars() {
        match state {
            State::LeadingWhitespace => {
                if !ch.is_whitespace() {
                    if ch.is_alphanumeric() {
                        state = State::Alnum;
                    } else {
                        state = State::NotAlnum;
                    }
                }
            }
            State::Alnum => {
                if !ch.is_alphanumeric() {
                    return pos;
                }
            }
            State::NotAlnum => {
                return pos;
            }
        }
        pos += ch.len_utf8();
    }
    pos
}

fn prev_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = offset;
    enum State {
        LeadingWhitespace,
        Alnum,
        NotAlnum,
    }
    let mut state = State::LeadingWhitespace;
    for ch in text[..offset].chars().rev() {
        match state {
            State::LeadingWhitespace => {
                if !ch.is_whitespace() {
                    if ch.is_alphanumeric() {
                        state = State::Alnum;
                    } else {
                        state = State::NotAlnum;
                    }
                }
            }
            State::Alnum => {
                if !ch.is_alphanumeric() {
                    return pos;
                }
            }
            State::NotAlnum => {
                return pos;
            }
        }
        pos -= ch.len_utf8();
    }
    pos
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Gesture {
    CharacterSelection,
    WordSelection { anchor: Selection },
}

/// Single- or multiline text editor.
pub struct TextEdit {
    element: Element,
    selection_changed: Handler<Selection>,
    state: RefCell<TextEditState>,
    gesture: Cell<Option<Gesture>>,
    blink_phase: Cell<bool>,
    blink_reset: Cell<bool>,
}

impl TextEdit {
    pub fn new() -> Rc<TextEdit> {
        let text_edit = Element::new_derived(|element| TextEdit {
            element,
            selection_changed: Handler::new(),
            state: RefCell::new(TextEditState {
                text: String::new(),
                selection: Selection::empty(0),
                text_style: TextStyle::default(),
                last_width_constraint: 0.0,
                paragraph: TextLayout::default().inner,
                selection_color: Color::from_rgba_u8(0, 0, 255, 80),
                caret_color: Color::from_rgba_u8(255, 255, 0, 255),
                relayout: true,
                scroll_offset: Vec2::new(0.0, 0.0),
                wrap_mode: WrapMode::Wrap,
                align: TextAlign::Start,
                size: Default::default(),
                text_overflow: TextOverflow::Clip,
                line_clamp: None,
            }),
            blink_phase: Cell::new(true),
            blink_reset: Cell::new(false),
            gesture: Cell::new(None),
        });

        text_edit.set_tab_focusable(true);

        // spawn the caret blinker task
        let this_weak = Rc::downgrade(&text_edit);
        spawn(async move {
            'task: loop {
                // Initial delay before blinking
                wait_for(CARET_BLINK_INITIAL_DELAY).await;
                // blinking
                'blink: loop {
                    if let Some(this) = this_weak.upgrade() {
                        if this.blink_reset.replace(false) {
                            // reset requested
                            this.blink_phase.set(true);
                            this.mark_needs_repaint();
                            break 'blink;
                        }
                        this.blink_phase.set(!this.blink_phase.get());
                        this.mark_needs_repaint();
                    } else {
                        // text edit is dead, exit task
                        break 'task;
                    }
                    wait_for(CARET_BLINK_INTERVAL).await;
                }
            }
        });

        text_edit
    }

    pub fn set_wrap_mode(&self, wrap_mode: WrapMode) {
        let this = &mut *self.state.borrow_mut();
        if this.wrap_mode != wrap_mode {
            this.wrap_mode = wrap_mode;
            this.rebuild_paragraph();
            this.relayout = true;
            self.mark_needs_relayout();
        }
    }

    pub fn set_max_lines(&self, max_lines: usize) {
        let this = &mut *self.state.borrow_mut();
        this.line_clamp = Some(max_lines);
        this.rebuild_paragraph();
        this.relayout = true;
        self.mark_needs_relayout();
    }

    pub fn set_text_align(&self, align: TextAlign) {
        let this = &mut *self.state.borrow_mut();
        if this.align != align {
            this.align = align;
            this.rebuild_paragraph();
            this.relayout = true;
            self.mark_needs_relayout();
        }
    }

    pub fn set_overflow(&self, overflow: TextOverflow) {
        let this = &mut *self.state.borrow_mut();
        if this.text_overflow != overflow {
            this.text_overflow = overflow;
            this.rebuild_paragraph();
            this.relayout = true;
            self.mark_needs_relayout();
        }
    }

    /// Resets the phase of the blinking caret.
    pub fn reset_blink(&self) {
        self.blink_phase.set(true);
        self.blink_reset.set(true);
        self.mark_needs_repaint();
    }

    pub fn set_caret_color(&self, color: Color) {
        let this = &mut *self.state.borrow_mut();
        if this.caret_color != color {
            this.caret_color = color;
            self.mark_needs_repaint();
        }
    }

    pub fn set_selection_color(&self, color: Color) {
        let this = &mut *self.state.borrow_mut();
        if this.selection_color != color {
            this.selection_color = color;
            self.mark_needs_repaint();
        }
    }

    pub fn set_text_style(&self, text_style: TextStyle) {
        let this = &mut *self.state.borrow_mut();
        this.text_style = text_style.into_static();
        this.rebuild_paragraph();
        this.relayout = true;
        self.mark_needs_relayout();
    }

    /// Returns the current selection.
    pub fn selection(&self) -> Selection {
        self.state.borrow().selection
    }

    /// Sets the current selection.
    pub fn set_selection(&self, selection: Selection) -> bool {
        // TODO clamp selection to text length
        let this = &mut *self.state.borrow_mut();
        if this.set_selection(selection) {
            self.mark_needs_repaint();
            true
        } else {
            false
        }
    }

    /// Returns the current text.
    pub fn text(&self) -> String {
        self.state.borrow().text.clone()
    }

    /// Sets the current text.
    pub fn set_text(&self, text: impl Into<String>) {
        // TODO we could compare the previous and new text
        // to relayout only affected lines.
        let this = &mut *self.state.borrow_mut();
        this.text = text.into();
        this.rebuild_paragraph();
        this.relayout = true;
        self.mark_needs_relayout();
    }

    pub fn text_position_for_point(&self, point: Point) -> usize {
        self.state.borrow().text_position_for_point(point)
    }

    /// NOTE: valid only after first layout.
    pub fn set_cursor_at_point(&self, point: Point, keep_anchor: bool) {
        if self.state.borrow_mut().set_cursor_at_point(point, keep_anchor) {
            self.mark_needs_repaint();
        }
    }

    pub fn select_word_under_cursor(&self) {
        if self.state.borrow_mut().select_word_under_cursor() {
            self.mark_needs_repaint();
        }
    }

    pub fn word_selection_at_text_position(&self, pos: usize) -> Selection {
        self.state.borrow().word_selection_at_text_position(pos)
    }

    /*pub fn select_word_at_offset_with_anchor(&self, offset: usize, anchor_selection: Selection) -> bool {
        let this = &mut *self.state.borrow_mut();
        let range = this.paragraph.get_word_boundary(offset as u32);
        let word = Selection {
            start: range.start,
            end: range.end,
        };

        // skia reports a word boundary for newlines, ignore it
        if this.text[range.clone()].starts_with('\n') {
            return false;
        }

        let new_selection = add_selections(anchor_selection, word);


        if new_selection != this.selection {
            this.selection = new_selection;
            this.scroll_in_view(this.selection.end);


            self.mark_needs_repaint();
            true
        } else {
            false
        }
    }*/

    /// Moves the cursor to the next or previous word boundary.
    pub fn move_cursor_to_next_word(&self, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_next_word(keep_anchor) {
            self.mark_needs_repaint();
        }
    }

    pub fn move_cursor_to_prev_word(&self, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_prev_word(keep_anchor) {
            self.mark_needs_repaint();
        }
    }

    pub fn move_cursor_to_next_grapheme(&self, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_next_grapheme(keep_anchor) {
            self.mark_needs_repaint();
        }
    }

    pub fn move_cursor_to_prev_grapheme(&self, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_prev_grapheme(keep_anchor) {
            self.mark_needs_repaint();
        }
    }

    /// Selects the line under the cursor.
    pub fn select_line_under_cursor(&self) {
        if self.state.borrow_mut().select_line_under_cursor() {
            self.mark_needs_repaint();
        }
    }

    /// Emitted when the selection changes as a result of user interaction.
    pub async fn selection_changed(&self) -> Selection {
        self.selection_changed.wait().await
    }
}

impl Deref for TextEdit {
    type Target = Element;

    fn deref(&self) -> &Self::Target {
        &self.element
    }
}

// Text view layout options:
// - alignment
// - wrapping
// - ellipsis (truncation mode)

// Given input constraints, and text that overflows:
// - wrap text
// - truncate text (with ellipsis)
// - become scrollable

impl ElementMethods for TextEdit {
    fn element(&self) -> &Element {
        &self.element
    }

    fn layout(&self, _children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> LayoutOutput {
        let this = &mut *self.state.borrow_mut();

        // layout first under infinite constraints to get the min/max intrinsic width
        this.paragraph.layout(f32::INFINITY);
        let min_line_width = this.paragraph.min_intrinsic_width();
        let max_line_width = this.paragraph.max_intrinsic_width();

        // TODO: support overflow clipping
        let width_constraint = layout_input.known_dimensions.width.unwrap_or_else(|| {
            match layout_input.available_space.width {
                AvailableSpace::Definite(width) => {
                    width.clamp(min_line_width, max_line_width)
                }
                AvailableSpace::MinContent => {
                    0.0
                }
                AvailableSpace::MaxContent => {
                    f32::INFINITY
                }
            }
        });

        //if this.relayout || this.last_width_constraint != width_constraint {
        this.paragraph.layout(width_constraint);
        //}
        this.relayout = false;
        this.last_width_constraint = width_constraint;

        // ceil to avoid clipping when relayouting the text
        let width = layout_input.known_dimensions.width.unwrap_or_else(|| this.paragraph.longest_line().ceil());
        let height = layout_input.known_dimensions.height.unwrap_or_else(|| this.paragraph.height());
        let baseline = this.paragraph.alphabetic_baseline();
        this.size = Size::new(width as f64, height as f64);

        LayoutOutput::from_sizes_and_baselines(taffy::Size { width, height }, taffy::Size::ZERO, taffy::Point {
            x: None,
            y: Some(baseline),
        })
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        let this = &mut *self.state.borrow_mut();
        let bounds = self.size().to_rect();

        ctx.with_canvas(|canvas| {
            // draw rect around bounds
            //let paint = Paint::from(Color::from_rgba_u8(255, 0, 0, 80)).to_sk_paint(bounds.to_rect());
            //canvas.draw_rect(bounds.to_rect().to_skia(), &paint);

            canvas.save();
            canvas.translate(-this.scroll_offset.to_skia());

            // paint the paragraph
            this.paragraph.paint(canvas, Point::ZERO.to_skia());

            // paint the selection rectangles
            let selection_rects = this.paragraph.get_rects_for_range(
                this.selection.min()..this.selection.max(),
                RectHeightStyle::Tight,
                RectWidthStyle::Tight,
            );
            let selection_paint = Paint::from(this.selection_color).to_sk_paint(bounds);
            for text_box in selection_rects {
                canvas.draw_rect(text_box.rect, &selection_paint);
            }

            if self.has_focus() && self.blink_phase.get() {
                if let Some(info) = this.paragraph.get_glyph_cluster_at(this.selection.end) {
                    let caret_rect = Rect::from_origin_size(
                        Point::new((info.bounds.left as f64).round(), (info.bounds.top as f64).round()),
                        Size::new(1.0, info.bounds.height() as f64),
                    );
                    //eprintln!("caret_rect: {:?}", caret_rect);
                    let caret_paint = Paint::from(this.caret_color).to_sk_paint(bounds);
                    canvas.draw_rect(caret_rect.to_skia(), &caret_paint);
                }
            }

            canvas.restore();
        });
    }

    async fn event(&self, event: &mut Event)
    where
        Self: Sized,
    {
        let mut selection_changed = false;
        let mut this = self.state.borrow_mut();
        let mut set_focus = false;

        match event {
            Event::PointerDown(event) => {
                let pos = event.local_position();
                eprintln!("[text_edit] pointer down: {:?}", pos);
                if event.repeat_count == 2 {
                    // select word under cursor
                    this.select_word_under_cursor();
                    selection_changed = true;
                    self.gesture.set(Some(Gesture::WordSelection {
                        anchor: this.selection,
                    }));
                } else if event.repeat_count == 3 {
                    // TODO select line under cursor
                } else {
                    selection_changed |= this.set_cursor_at_point(pos, false);
                    self.gesture.set(Some(Gesture::CharacterSelection));
                }
                self.reset_blink();
                // Don't immediately call `set_focus` because we'll recurse into this event handler
                // with `self.state` already borrowed mutably.
                set_focus = true;
                self.set_pointer_capture();
            }
            Event::PointerMove(event) => {
                //eprintln!("pointer move point: {:?}", event.local_position());
                let pos = event.local_position();

                match self.gesture.get() {
                    Some(Gesture::CharacterSelection) => {
                        selection_changed |= this.set_cursor_at_point(pos, true);
                    }
                    Some(Gesture::WordSelection { anchor }) => {
                        let text_offset = this.text_position_for_point(pos);
                        let word_selection = this.word_selection_at_text_position(text_offset);
                        selection_changed |= this.set_selection(add_selections(anchor, word_selection));
                    }
                    _ => {}
                }

                self.reset_blink();
            }
            Event::PointerUp(_event) => {
                self.gesture.set(None);
            }
            Event::FocusGained => {
                eprintln!("focus gained");
                self.reset_blink();
            }
            Event::FocusLost => {
                eprintln!("focus lost");
                selection_changed |= this.set_selection(Selection::empty(0));
            }
            Event::KeyDown(event) => {
                let keep_anchor = event.modifiers.shift();
                let word_nav = event.modifiers.ctrl();
                match event.key {
                    Key::ArrowLeft => {
                        // TODO bidi?
                        if word_nav {
                            this.move_cursor_to_prev_word(keep_anchor);
                        } else {
                            this.move_cursor_to_prev_grapheme(keep_anchor);
                        }
                        selection_changed = true;
                        self.reset_blink();
                    }
                    Key::ArrowRight => {
                        if word_nav {
                            this.move_cursor_to_next_word(keep_anchor);
                        } else {
                            this.move_cursor_to_next_grapheme(keep_anchor);
                        }
                        selection_changed = true;
                        self.reset_blink();
                    }
                    Key::Character(ref s) => {
                        // TODO don't do this, emit the changed text instead
                        let mut text = this.text.clone();
                        let selection = this.selection;
                        text.replace_range(selection.byte_range(), &s);
                        this.text = text;
                        this.rebuild_paragraph();
                        this.relayout = true;
                        this.selection = Selection::empty(selection.min() + s.len());
                        selection_changed = true;
                        self.mark_needs_relayout();
                        self.reset_blink();
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        drop(this);

        if set_focus {
            self.set_focus().await;
        }

        if selection_changed {
            self.mark_needs_repaint();
            self.selection_changed.emit(self.selection()).await;
        }
    }
}
