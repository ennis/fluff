use tracing::trace;
use crate::layout::{LayoutInput, LayoutMode, LayoutOutput, SizeConstraint};

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LayoutCacheEntry {
    // Width and height specified and finite
    FullySpecified = 0,
    // Width=inf, height whatever
    WidthInfinite,
    Count,
}

/// Layout cache.
#[derive(Default)]
pub struct LayoutCache {
    entries: [Option<(LayoutInput, LayoutOutput)>; LayoutCacheEntry::Count as usize],
}

impl LayoutCache {
    pub fn get_cached(&self, entry: LayoutCacheEntry, input: &LayoutInput) -> Option<LayoutOutput> {
        let Some((cached_input, cached_output)) = self.entries[entry as usize] else {
            // no entry in cache
            return None;
        };

        if cached_input == *input {
            // exact match
            return Some(cached_output);
        }

        let (
            SizeConstraint::Available(w),
            SizeConstraint::Available(h),
            SizeConstraint::Available(cached_w),
            SizeConstraint::Available(cached_h),
        ) = (input.width, input.height, cached_input.width, cached_input.height)
        else {
            return None;
        };

        // if we returned a box of size w x h for request W1 x H1, and now we're asked for W2 x H2,
        // with w < W2 < W1  and h < H2 < H1, we can still use the cached layout
        // (the new box is smaller but the previous result still fits inside)
        // We can't do that if the new request is larger than the previous one, because given a larger
        // request the element might choose to layout itself differently.

        if cached_output.width <= w && w <= cached_w && cached_output.height <= h && h <= cached_h {
            return Some(cached_output);
        }

        // No match
        None
    }

    pub fn get_or_insert_with(
        &mut self,
        layout_input: &LayoutInput,
        mode: LayoutMode,
        f: impl FnOnce(&LayoutInput) -> LayoutOutput,
    ) -> LayoutOutput {
        if mode == LayoutMode::Place {
            return f(layout_input);
        }

        let entry_index = match layout_input {
            LayoutInput {
                width: SizeConstraint::Available(w),
                height: SizeConstraint::Available(h),
                ..
            } if w.is_finite() && h.is_finite() => LayoutCacheEntry::FullySpecified,
            LayoutInput {
                width: SizeConstraint::Available(w),
                ..
            } if *w == f64::INFINITY => LayoutCacheEntry::WidthInfinite,
            _ => LayoutCacheEntry::Count,
        };

        if entry_index < LayoutCacheEntry::Count {
            if let Some(layout) = self.get_cached(entry_index, layout_input) {
                trace!("using cached layout for entry {entry_index:?}: {layout:?}");
                return layout;
            }
            let output = f(layout_input);
            self.entries[entry_index as usize] = Some((*layout_input, output));
            output
        } else {
            f(layout_input)
        }
    }
}