//! Layout units.

use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

const LU_PER_PX: i32 = 60;


/// "App Units": a length in 1/60ths of pixels.
///
/// https://doc.servo.org/app_units/struct.Au.html
#[repr(transparent)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lu(pub i32);

impl Lu {
    /// This length in pixels, rounded towards zero.
    pub const fn px(self) -> i32 {
        self.0 / LU_PER_PX
    }

    pub const fn from_px(px: i32) -> Self {
        Lu(px * LU_PER_PX)
    }
}

impl Add for Lu {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Lu(self.0 + rhs.0)
    }
}

impl AddAssign for Lu {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Lu {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Lu(self.0 - rhs.0)
    }
}

impl SubAssign for Lu {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<i32> for Lu {
    type Output = Self;

    fn mul(self, rhs: i32) -> Self {
        Lu(self.0 * rhs)
    }
}

impl Mul<Lu> for i32 {
    type Output = Lu;

    fn mul(self, rhs: Lu) -> Lu {
        Lu(self * rhs.0)
    }
}

impl MulAssign<i32> for Lu {
    fn mul_assign(&mut self, rhs: i32) {
        self.0 *= rhs;
    }
}

impl Div<i32> for Lu {
    type Output = Self;

    fn div(self, rhs: i32) -> Self {
        Lu(self.0 / rhs)
    }
}

impl DivAssign<i32> for Lu {
    fn div_assign(&mut self, rhs: i32) {
        self.0 /= rhs;
    }
}

pub type LuSize = euclid::Size2D<Lu, euclid::UnknownUnit>;
pub type LuVec2 = euclid::Vector2D<Lu, euclid::UnknownUnit>;
pub type LuRect = euclid::Rect<Lu, euclid::UnknownUnit>;
//pub type LuInsets = euclid::

/*
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LuVec2 {
    pub x: Lu,
    pub y: Lu,
}

impl LuVec2 {
    pub const fn new(x: Lu, y: Lu) -> Self {
        Self { x, y }
    }
}

impl Add for LuVec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign for LuVec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for LuVec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign for LuVec2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<i32> for LuVec2 {
    type Output = Self;

    fn mul(self, rhs: i32) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Mul<LuVec2> for i32 {
    type Output = LuVec2;

    fn mul(self, rhs: LuVec2) -> LuVec2 {
        LuVec2 {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}

impl MulAssign<i32> for LuVec2 {
    fn mul_assign(&mut self, rhs: i32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<i32> for LuVec2 {
    type Output = Self;

    fn div(self, rhs: i32) -> Self {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl DivAssign<i32> for LuVec2 {
    fn div_assign(&mut self, rhs: i32) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LuSize {
    pub width: Lu,
    pub height: Lu,
}

impl Add for LuSize {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            width: self.width + rhs.width,
            height: self.height + rhs.height,
        }
    }
}

impl AddAssign for LuSize {
    fn add_assign(&mut self, rhs: Self) {
        self.width += rhs.width;
        self.height += rhs.height;
    }
}

impl Sub for LuSize {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            width: self.width - rhs.width,
            height: self.height - rhs.height,
        }
    }
}

impl SubAssign for LuSize {
    fn sub_assign(&mut self, rhs: Self) {
        self.width -= rhs.width;
        self.height -= rhs.height;
    }
}

impl Mul<i32> for LuSize {
    type Output = Self;

    fn mul(self, rhs: i32) -> Self {
        Self {
            width: self.width * rhs,
            height: self.height * rhs,
        }
    }
}

impl Mul<LuSize> for i32 {
    type Output = LuSize;

    fn mul(self, rhs: LuSize) -> LuSize {
        LuSize {
            width: self * rhs.width,
            height: self * rhs.height,
        }
    }
}

impl MulAssign<i32> for LuSize {
    fn mul_assign(&mut self, rhs: i32) {
        self.width *= rhs;
        self.height *= rhs;
    }
}

impl Div<i32> for LuSize {
    type Output = Self;

    fn div(self, rhs: i32) -> Self {
        Self {
            width: self.width / rhs,
            height: self.height / rhs,
        }
    }
}

impl DivAssign<i32> for LuSize {
    fn div_assign(&mut self, rhs: i32) {
        self.width /= rhs;
        self.height /= rhs;
    }
}
*/