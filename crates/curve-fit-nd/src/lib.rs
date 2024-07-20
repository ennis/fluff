/*
#[doc = " Takes a flat array of points and evaluates that to calculate a bezier spline.\n\n \\param points, points_len: The array of points to calculate a cubics from.\n \\param dims: The number of dimensions for each element in \\a points.\n \\param error_threshold: the error threshold to allow for,\n the curve will be within this distance from \\a points.\n \\param corners, corners_len: indices for points which will not have aligned tangents (optional).\n This can use the output of #curve_fit_corners_detect_db which has been included\n to evaluate a line to detect corner indices.\n\n \\param r_cubic_array, r_cubic_array_len: Resulting array of tangents and knots, formatted as follows:\n `r_cubic_array[r_cubic_array_len][3][dims]`,\n where each point has 0 and 2 for the tangents and the middle index 1 for the knot.\n The size of the *flat* array will be `r_cubic_array_len * 3 * dims`.\n \\param r_corner_index_array, r_corner_index_len: Corner indices in \\a r_cubic_array (optional).\n This allows you to access corners on the resulting curve.\n\n \\returns zero on success, nonzero is reserved for error values."]
pub fn curve_fit_cubic_to_points_db(
    points: *const f64,
    points_len: ::std::os::raw::c_uint,
    dims: ::std::os::raw::c_uint,
    error_threshold: f64,
    calc_flag: ::std::os::raw::c_uint,
    corners: *const ::std::os::raw::c_uint,
    corners_len: ::std::os::raw::c_uint,
    r_cubic_array: *mut *mut f64,
    r_cubic_array_len: *mut ::std::os::raw::c_uint,
    r_cubic_orig_index: *mut *mut ::std::os::raw::c_uint,
    r_corner_index_array: *mut *mut ::std::os::raw::c_uint,
    r_corner_index_len: *mut ::std::os::raw::c_uint,
) -> ::std::os::raw::c_int;

*/

use bitflags::bitflags;
use std::{ptr, slice};

bitflags! {
    pub struct CalcFlags: u32 {
        const HIGH_QUALITY = curve_fit_nd_sys::CURVE_FIT_CALC_HIGH_QUALIY as u32;
        const CYCLIC = curve_fit_nd_sys::CURVE_FIT_CALC_CYCLIC as u32;
    }
}

/// Result of the `curve_fit_cubic_to_points_db` function.
pub struct CurveFitCubicResult {
    /// Resulting array of tangents and knots, formatted as follows `r_cubic_array[r_cubic_array_len][3][dims]`
    pub cubic_array: Vec<f64>,
    pub corner_index_array: Option<Vec<u32>>,
    pub cubic_orig_index: Option<Vec<u32>>,
}

unsafe fn buffer_into_vec<T>(buffer: *mut T, len: usize) -> Option<Vec<T>> {
    if buffer.is_null() {
        return None;
    }
    let vec = slice::from_raw_parts(buffer, len).to_vec();
    libc::free(buffer as *mut libc::c_void);
    Some(vec)
}

/// Takes a flat array of points and evaluates that to calculate a bezier spline.
///
/// # Parameters
/// * points: The array of points to calculate a cubics from.\n
/// * dims: The number of dimensions for each element in \\a points.
/// * error_threshold: the error threshold to allow for,\n the curve will be within this distance from \\a points.\n
/// * corners, corners_len: indices for points which will not have aligned tangents (optional).\n This can use the output of #curve_fit_corners_detect_db which has been included\n to evaluate a line to detect corner indices.\n\n
/// * r_cubic_array: Resulting array of tangents and knots, formatted as follows:\n `r_cubic_array[r_cubic_array_len][3][dims]`,\n where each point has 0 and 2 for the tangents and the middle index 1 for the knot.\n
/// The size of the *flat* array will be `r_cubic_array_len * 3 * dims`.\n
/// * r_corner_index_array, r_corner_index_len: Corner indices in \\a r_cubic_array (optional).\n This allows you to access corners on the resulting curve.\n\n
///
/// # Return value
/// Ok(()) on success, nonzero is reserved for error values.

pub fn curve_fit_cubic_to_points_f64(
    points: &[f64],
    dims: usize,
    error_threshold: f64,
    calc_flag: CalcFlags,
    corners: Option<&[u32]>,
) -> Result<CurveFitCubicResult, u32> {
    let mut r_cubic_array = ptr::null_mut();
    let mut r_cubic_array_len = 0;
    let mut r_cubic_orig_index = ptr::null_mut();
    let mut r_corner_index_array = ptr::null_mut();
    let mut r_corner_index_len = 0;

    unsafe {
        let r = curve_fit_nd_sys::curve_fit_cubic_to_points_db(
            points.as_ptr(),
            points.len() as u32,
            dims as u32,
            error_threshold,
            calc_flag.bits(),
            corners.map(|c| c.as_ptr()).unwrap_or(ptr::null()),
            corners.map(|c| c.len() as u32).unwrap_or(0),
            &mut r_cubic_array,
            &mut r_cubic_array_len,
            &mut r_cubic_orig_index,
            &mut r_corner_index_array,
            &mut r_corner_index_len,
        );

        let cubic_array =
            buffer_into_vec(r_cubic_array, r_cubic_array_len as usize).expect("curve_fit_cubic_to_points_db returned null pointer");
        let corner_index_array = buffer_into_vec(r_corner_index_array, r_corner_index_len as usize);
        let cubic_orig_index = buffer_into_vec(r_cubic_orig_index, r_cubic_array_len as usize);

        Ok(CurveFitCubicResult {
            cubic_array,
            corner_index_array,
            cubic_orig_index,
        })
    }
}
