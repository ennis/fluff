use std::ops::Range;

/// Text selection.
///
/// Start is the start of the selection, end is the end. The caret is at the end of the selection.
/// Note that we don't necessarily have start <= end: a selection with start > end means that the
/// user started the selection gesture from a later point in the text and then went back
/// (right-to-left in LTR languages). In this case, the cursor will appear at the "beginning"
/// (i.e. left, for LTR) of the selection.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Selection {
    pub start: usize,
    /// The end of the selection, and also the position of the caret. Not necessarily greater than start,
    /// if the selection was made by dragging from right to left.
    pub end: usize,
}

impl Selection {
    pub fn min(&self) -> usize {
        self.start.min(self.end)
    }
    pub fn max(&self) -> usize {
        self.start.max(self.end)
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
    pub fn empty(at: usize) -> Selection {
        Selection { start: at, end: at }
    }
    pub fn byte_range(&self) -> Range<usize> {
        self.min()..self.max()
    }
    pub fn clamp(self, range: Range<usize>) -> Selection {
        let start = self.start.clamp(range.start, range.end);
        let end = self.end.clamp(range.start, range.end);
        Selection { start, end }
    }
}

impl Default for Selection {
    fn default() -> Self {
        Selection::empty(0)
    }
}
