//! f32 with NaN payload.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// A value that represents either a 32-bit floating point value or an index into the
/// variable table.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct PackF32(pub u32);

const NAN_MASK: u32 = 0x7FC00000;
const NAN_BITS: u32 = 0x7FC00000;
const PAYLOAD_MASK_BITS: u32 = 22;
const PAYLOAD_MASK: u32 = (1 << PAYLOAD_MASK_BITS) - 1;

impl PackF32 {
    /// Returns this value as a f32.
    pub fn as_f32(self) -> f32 {
        f32::from_bits(self.0)
    }

    /// Returns whether this value is a NaN holding a payload.
    pub fn is_index(self) -> bool {
        self.0 & NAN_MASK == NAN_BITS
    }

    pub fn index(self) -> Option<u16> {
        if self.is_index() {
            Some((self.0 & PAYLOAD_MASK) as u16)
        } else {
            None
        }
    }

    /// Creates a new packed f32 with the given 22-bit payload in the NaN.
    pub fn from_index(payload: u32) -> PackF32 {
        assert!(payload <= PAYLOAD_MASK, "payload out of range");
        PackF32(NAN_BITS | payload)
    }

    /// Creates a new packed f32 with the given f32 value.
    pub fn from_f32(value: f32) -> PackF32 {
        assert!(!value.is_nan(), "NaN not allowed");
        PackF32(value.to_bits())
    }
}