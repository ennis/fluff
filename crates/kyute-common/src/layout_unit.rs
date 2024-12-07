//! Visual length units.

const AU_PER_PX: f32 = 96.0 / 72.0;

/// "App Units": a length in 1/60ths of pixels.
/// 
/// https://doc.servo.org/app_units/struct.Au.html
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Au(pub i32);

impl Au {
    /// This length in pixels, rounded towards zero.
    pub fn px(self) -> i32 {
        (self.0 as f32 / 60.0f32 as i32
    }


}