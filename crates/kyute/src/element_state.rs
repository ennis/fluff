use bitflags::bitflags;

bitflags! {
    /// Flags describing the state of an interactive element.
    #[derive(Copy, Clone, Debug, Default)]
    pub struct ElementState: u8 {
        /// The element is active (e.g. pressed).
        const ACTIVE = 0b0001;
        /// The mouse is hovering over the element.
        const HOVERED = 0b0010;
        /// The element has focus.
        const FOCUSED = 0b0100;
    }
}

impl ElementState {
    /// Sets the `ACTIVE` flag.
    pub fn set_active(&mut self, active: bool) {
        self.set(ElementState::ACTIVE, active);
    }

    /// Sets the `HOVERED` flag.
    pub fn set_hovered(&mut self, hovered: bool) {
        self.set(ElementState::HOVERED, hovered);
    }

    /// Sets the `FOCUSED` flag.
    pub fn set_focused(&mut self, focused: bool) {
        self.set(ElementState::FOCUSED, focused);
    }

    /// Returns `true` if the `ACTIVE` flag is set.
    pub fn is_active(&self) -> bool {
        self.contains(ElementState::ACTIVE)
    }

    /// Returns `true` if the `HOVERED` flag is set.
    pub fn is_hovered(&self) -> bool {
        self.contains(ElementState::HOVERED)
    }

    /// Returns `true` if the `FOCUSED` flag is set.
    pub fn is_focused(&self) -> bool {
        self.contains(ElementState::FOCUSED)
    }
}