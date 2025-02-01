//! Macro to create styled text runs.

use crate::element::{ElementAny, IntoElementAny, WeakElementAny};
use crate::elements::text::Text;
use crate::text::TextStyle;

/// String slice with associated style attributes.
#[derive(Copy, Clone)]
pub struct TextRun<'a> {
    pub str: &'a str,
    pub style: &'a TextStyle<'a>,
}

#[doc(hidden)]
#[macro_export]
macro_rules! __text {
    // Parse styles
    (@style($s:ident) rgb ($($p:expr),*) ) => {
        $s.color = $crate::Color::from_rgb_u8($($p),*);
    };

    (@style($s:ident) color ($f:expr) ) => {
        $s.color = $f;
    };

    (@style($s:ident) hexcolor ($f:expr) ) => {
        $s.color = $crate::Color::from_hex($f);
    };

    (@style($s:ident) i ) => {
        $s.font_style = $crate::text::FontStyle::Italic;
    };

    (@style($s:ident) b ) => {
        $s.font_weight = $crate::text::FontWeight::BOLD;
    };

    (@style($s:ident) family ($f:expr) ) => {
        $s.font_family = $f.into();
    };

    (@style($s:ident) size ($f:expr) ) => {
        $s.font_size = $f.into();
    };

    (@style($s:ident) weight ($f:expr) ) => {
        $s.font_weight = $crate::text::FontWeight::parse($f);
    };

    (@style($s:ident) width ($f:expr) ) => {
        $s.font_stretch = $crate::text::FontStretch::parse($f)
    };

    (@style($s:ident) oblique ) => {
        $s.font_style = $crate::text::FontStyle::Oblique;
    };

    (@style($s:ident) style ($f:expr) ) => {
        $s = $f.clone();
    };

    (@style($s:ident) $($rest:tt)*) => {
        compile_error!("Unrecognized style property");
    };

    /////////////////////////////////
    // style stack reverser
    (@apply_styles($s:ident) ) => {};

    (@apply_styles($s:ident) ( $(($($styles:tt)*))* ) $($rest:tt)* ) => {
         $crate::__text!(@apply_styles($s) $($rest)*);
         $($crate::__text!(@style($s) $($styles)*);)*
    };

    ////////////////////
    // finish rule
    (
        // input
        ()

        // style stack, unused here
        ($($sty:tt)*)
        // string parts with their associated styles
        // TODO document format of styles
        ($(
            ($part:literal, $($styles:tt)* )
        )*)
    ) => {
        &[
            $(
            $crate::text::TextRun {
                str: &*$crate::text::cow_format_args(::std::format_args!($part)),
                style: &{
                    let mut __s = $crate::text::TextStyle::default();
                    $crate::__text!(@apply_styles(__s) $($styles)*);
                    __s
                }
            },
            )*
        ]
    };

    ////////////////////
    // pop style
    (
        // input
        ( @pop $($r:tt)* )
        // output
        ( ($($sty_top:tt)*) $(($($sty_rest:tt)*))* )
        ($($ranges:tt)*)
    ) => {
        $crate::__text!(
            ($($r)*)
            ($(($($sty_rest)*))*)
            ($($ranges)*)
        )
    };

    ////////////////////
    // string literal
    (
        // input
        ( $str:literal $($r:tt)* )
        // output
        ( $(($($sty:tt)*))*)
        ( $($ranges:tt)* )

    ) => {
        $crate::__text!(
            ($($r)*)
            ( $(($($sty)*))*)
            ( $($ranges)* ( $str, $(($($sty)*))* ))
        )
    };

    ////////////////////
    // style modifier
    (
        // input
        ( $m:ident ($($mp:expr),*) $($r:tt)* )
        // output
        ( ($($cur_style:tt)*) $($style_stack:tt)*)
        ( $($ranges:tt)* )
    ) => {
        $crate::__text!(
            ($($r)*)
            ( ( $($cur_style)* ($m ($($mp),*)) ) $($style_stack)*)
            ($($ranges)*)
        )
    };

    ////////////////////
    // style modifier
    (
        // input
        ( $m:ident $($r:tt)* )
        // output
        ( ($($cur_style:tt)*) $($style_stack:tt)*)
        ($($ranges:tt)*)
    ) => {

        $crate::__text!(
            ($($r)*)
            ( ( $($cur_style)* ($m) ) $($style_stack)*)
            ($($ranges)*)
        )
    };

    ////////////////////
    // color modifier (literal ver.)
    (
        // input
        ( # $color:literal $($r:tt)* )
        // output
        ($($style_stack:tt)*)
        ($($ranges:tt)*)
    ) => {

        $crate::__text!(
            (hexcolor(::std::stringify!($color)) $($r)*)
            ($($style_stack)*)
            ($($ranges)*)
        )
    };


    ////////////////////
    // color modifier (ident ver. when the color starts with a letter...)
    (
        // input
        ( # $color:ident $($r:tt)* )
        // output
        ($($style_stack:tt)*)
        ($($ranges:tt)*)
    ) => {

        $crate::__text!(
            (hexcolor(::std::stringify!($color)) $($r)*)
            ($($style_stack)*)
            ($($ranges)*)
        )
    };


    ////////////////////
    // block start
    (
        // input
        ( { $($inner:tt)* } $($r:tt)* )
        // output
        ($($style_stack:tt)*)
        ($($ranges:tt)*)
    )
    => {
        $crate::__text!(
            ( $($inner)* @pop $($r)* )
            (() $($style_stack)*)
            ($($ranges)*)
        )
    };

    /*(@body($runs:ident,$style:ident) $str:literal $($rest:tt)*) => {
        runs.push($crate::text::AttributedRange::owned(format!($str), $style.clone()));
        __text! { @body($runs,$style) $($rest)* };
    };*/
}

/// Macro that expands to an array of styled `TextRun`s (`&[TextRun<'_>]`).
///
/// Note that this macro, like `format_args!`, borrows temporaries, so you may not be able to assign
/// the result to a variable. However, you can use it with methods that accept &[TextRun<'_>].
///
/// # Syntax
/// TODO
///
/// # Example
///
/// ```
/// use kyute::text;
///
/// text! [ size(20.0) "Hello, world!" { b "test" } ];
/// ```
#[macro_export]
macro_rules! text {
    [ $($rest:tt)* ] => {
        {
            $crate::__text!(
                ( $($rest)* )
                (())
                ()
            )
        }
    };
}

impl<const N: usize> IntoElementAny for &[TextRun<'_>; N] {
    fn into_element(self, parent: WeakElementAny, index_in_parent: usize) -> ElementAny {
        Text::new(self).into_element(parent, index_in_parent)
    }
}
