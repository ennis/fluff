//! Macro to create styled text runs.

use crate::{IntoNode, RcNode, WeakDynNode};
use crate::elements::text::Text;
use crate::text::StyleProperty;
use crate::window::WindowHandle;

/// String slice with associated style attributes.
#[derive(Copy, Clone)]
pub struct TextRun<'a> {
    pub str: &'a str,
    pub styles: &'a [StyleProperty<'a>],
}

#[doc(hidden)]
#[macro_export]
macro_rules! __text {
    /*// Parse styles
    (@style($s:ident) rgb ($($p:expr),*) ) => {
        $crate::text::StyleProperty::Color($crate::Color::from_rgb_u8($($p),*)),
    };

    (@style($s:ident) color ($f:expr) ) => {
        $crate::text::StyleProperty::Color($f),
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
    };*/

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
            ($part:literal, $(($($styles:tt)*))* )
        )*)
    ) => {
        &[
            $(
            $crate::text::TextRun {
                str: &*$crate::text::cow_format_args(::std::format_args!($part)),
                styles: &[
                    $($crate::text::StyleProperty::$($styles)*),*
                ],
                //    let mut __s = $crate::text::TextStyle::default();
                //    $crate::__text!(@apply_styles(__s) $($styles)*);
                //    __s
                //}
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
        ( ($($sty_top:tt)*) $($sty_rest:tt)* )
        ($($ranges:tt)*)
    ) => {
        $crate::__text!(
            ($($r)*)
            ($($sty_rest)*)
            ($($ranges)*)
        )
    };

    ////////////////////
    // string literal
    (
        // input
        ( $str:literal $($r:tt)* )
        // output
        ( ($($sty_top:tt)*) $($sty_rest:tt)* )
        ( $($ranges:tt)* )

    ) => {
        $crate::__text!(
            ($($r)*)
            ( ( $($sty_top)* ) $($sty_rest)* )
            ( $($ranges)* ( $str, $($sty_top)* ) )
        )
    };

    ////////////////////
    // style modifier
    (
        // input
        ( $m:ident ($($mp:expr),*) $($r:tt)* )
        // output
        ( ($($sty_top:tt)*) $($sty_rest:tt)*)
        ( $($ranges:tt)* )
    ) => {
        $crate::__text!(
            ($($r)*)
            ( ( $($sty_top)* ($m ($($mp),*)) ) $($sty_rest)* )
            ($($ranges)*)
        )
    };

    ////////////////////
    // style modifier
    (
        // input
        ( $m:ident $($r:tt)* )
        // output
        ( ($($sty_top:tt)*) $($sty_rest:tt)*)
        ($($ranges:tt)*)
    ) => {

        $crate::__text!(
            ($($r)*)
            ( ( $($sty_top)* ($m) ) $($sty_rest)* )
            ($($ranges)*)
        )
    };

    ////////////////////
    // block start
    (
        // input
        ( { $($inner:tt)* } $($r:tt)* )

        // output
        // ( $( ( $( ($style) )*) )* )
        ( ($($sty_top:tt)*) $($sty_rest:tt)*)
        ($($ranges:tt)*)
    )
    => {
        $crate::__text!(
            ( $($inner)* @pop $($r)* )
            // duplicate the top of the stack
            (($($sty_top)*) ($($sty_top)*) $($sty_rest)*)
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
/// the result to a variable. However, you can use it with methods that accept `&[TextRun<'_>]`.
///
/// # Syntax
/// TODO
///
/// # Example
///
/// ```
/// use kyute::text;
///
/// text! [ FontSize(20.) "Hello, world!" { FontWeight(FontWeight::BOLD) "test" } ];
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

/*fn test() {
    text! [ FontSize(20.0) "Hello, world!" { FontWeight() "test" } ];
}*/

impl<const N: usize> IntoNode for &[TextRun<'_>; N] {
    type Element = Text;
    fn into_node(self, parent: WeakDynNode) -> RcNode<Text> {
        Text::new(self).into_node(parent)
    }

    fn into_root_node(self, parent_window: WindowHandle) -> RcNode<Self::Element> {
        Text::new(self).into_root_node(parent_window)
    }
}
