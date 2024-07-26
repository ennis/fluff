use std::{f32::consts::PI, f64::consts::TAU};
use std::cell::Cell;

use glam::{dvec2, DVec2, DVec3, dvec3, Mat4, vec3, Vec3, Vec3Swizzles, Vec4Swizzles};
use tracing::debug;
use winit::event::MouseButton;

use crate::aabb::AABB;

#[derive(Copy, Clone, Debug, Default)]
pub struct Frustum {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    // near clip plane position
    pub near_plane: f32,
    // far clip plane position
    pub far_plane: f32,
}

/// Represents a camera (a view of a scene).
#[derive(Copy, Clone, Debug)]
pub struct Camera {
    // Projection parameters
    // frustum (for culling)
    pub frustum: Frustum,
    // view matrix
    // (World -> View)
    pub view: Mat4,
    pub view_inverse: Mat4,
    // projection matrix
    // (View -> clip?)
    pub projection: Mat4,
    pub projection_inverse: Mat4,
    pub screen_size: DVec2,
}

impl Camera {
    pub fn view_projection(&self) -> Mat4 {
        self.projection * self.view
    }

    pub fn screen_to_ndc(&self, screen_pos: DVec3) -> DVec3 {
        // Note: Vulkan NDC space (depth 0->1) is different from OpenGL  (-1 -> 1)
        self.screen_to_ndc_2d(screen_pos.xy()).extend(screen_pos.z)
    }

    pub fn screen_to_ndc_2d(&self, screen_pos: DVec2) -> DVec2 {
        dvec2(
            2.0 * screen_pos.x / self.screen_size.x - 1.0,
            1.0 - 2.0 * screen_pos.y / self.screen_size.y,
        )
    }

    /// Unprojects a screen-space position to a view-space ray direction.
    ///
    /// This assumes a normalized depth range of `[0, 1]`.
    pub fn screen_to_view(&self, screen_pos: DVec3) -> DVec3 {
        // Undo viewport transformation
        let ndc = self.screen_to_ndc(screen_pos).as_vec3();
        // TODO matrix ops as f64?
        let inv_proj = self.projection.inverse();
        let clip = inv_proj * ndc.extend(1.0);
        (clip.xyz() / clip.w).as_dvec3()
    }

    /// Unprojects a screen-space position to a view-space ray direction.
    ///
    /// This assumes a normalized depth range of `[0, 1]`.
    pub fn screen_to_view_dir(&self, screen_pos: DVec2) -> DVec3 {
        self.screen_to_view(dvec3(screen_pos.x, screen_pos.y, 0.0)).normalize()
    }

    pub fn screen_to_world(&self, screen_pos: DVec3) -> DVec3 {
        let view_pos = self.screen_to_view(screen_pos).as_vec3();
        let world_pos = self.view.inverse() * view_pos.extend(1.0);
        world_pos.xyz().as_dvec3()
    }

    pub fn eye(&self) -> DVec3 {
        self.view_inverse.transform_point3(Vec3::ZERO).as_dvec3()
    }

    pub fn screen_to_world_ray(&self, screen_pos: DVec2) -> (DVec3, DVec3) {
        let world_pos = self.screen_to_world(screen_pos.extend(0.0));
        let eye_pos = self.view_inverse.transform_point3(Vec3::ZERO).as_dvec3();
        (eye_pos, (world_pos - eye_pos).normalize())
    }
}

impl Default for Camera {
    fn default() -> Self {
        let view = Mat4::look_at_rh(vec3(0.0, 0.0, -1.0), vec3(0.0, 0.0, 0.0), Vec3::Y);
        let view_inverse = view.inverse();
        let projection = Mat4::perspective_rh(PI / 2.0, 1.0, 0.01, 10.0);
        let projection_inverse = projection.inverse();

        Camera {
            // TODO
            frustum: Default::default(),
            view,
            view_inverse,
            projection,
            projection_inverse,
            screen_size: Default::default(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct CameraFrame {
    eye: DVec3,
    up: DVec3,
    center: DVec3,
}

#[derive(Copy, Clone, Debug)]
enum CameraInputMode {
    None,
    Pan { anchor_screen: DVec2, orig_frame: CameraFrame },
    Tumble { anchor_screen: DVec2, orig_frame: CameraFrame },
}

/// A camera controller that generates `Camera` instances.
///
/// TODO describe parameters
#[derive(Clone, Debug)]
pub struct CameraControl {
    fov_y_radians: f64,
    z_near: f64,
    z_far: f64,
    zoom: f32,
    screen_size: DVec2,
    cursor_pos: Option<DVec2>,
    frame: CameraFrame,
    input_mode: CameraInputMode,
    last_cam: Cell<Option<Camera>>,
}

#[derive(Copy, Clone, Debug)]
pub enum CameraControlInput {
    MouseInput { button: MouseButton, pressed: bool },
    CursorMoved { position: DVec2 },
}

impl CameraControl {
    /// Creates the camera controller state.
    ///
    /// # Arguments
    /// - `width` width of the screen in physical pixels
    /// - `height` height of the screen in physical pixels
    pub fn new(width: u32, height: u32) -> CameraControl {
        CameraControl {
            fov_y_radians: std::f64::consts::PI / 2.0,
            z_near: 0.1,
            z_far: 10.0,
            zoom: 1.0,
            screen_size: dvec2(width as f64, height as f64),
            cursor_pos: None,
            frame: CameraFrame {
                eye: glam::dvec3(0.0, 0.0, 2.0),
                up: glam::dvec3(0.0, 1.0, 0.0),
                center: glam::dvec3(0.0, 0.0, 0.0),
            },
            input_mode: CameraInputMode::None,
            last_cam: Cell::new(None),
        }
    }

    /// Call when the size of the screen changes.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.screen_size = dvec2(width as f64, height as f64);
        self.last_cam.set(None);
    }

    /// Returns the current eye position.
    pub fn eye(&self) -> DVec3 {
        self.frame.eye
    }

    fn handle_pan(&mut self, orig: &CameraFrame, delta_screen: glam::DVec2) {
        let delta = delta_screen / self.screen_size;
        let dir = orig.center - orig.eye;
        let right = dir.normalize().cross(orig.up);
        let dist = dir.length();
        self.frame.eye = orig.eye + dist * (-delta.x * right + delta.y * orig.up);
        self.frame.center = orig.center + dist * (-delta.x * right + delta.y * orig.up);
        self.last_cam.set(None);
    }

    fn to_ndc(&self, p: glam::DVec2) -> DVec2 {
        2.0 * (p / self.screen_size) - dvec2(1.0, 1.0)
    }

    fn handle_tumble(&mut self, orig: &CameraFrame, from: DVec2, to: DVec2) {
        let delta = (to - from) / self.screen_size;
        let eye_dir = orig.eye - orig.center;
        let right = eye_dir.normalize().cross(orig.up);
        let r = glam::DQuat::from_rotation_y(-delta.x * TAU) * glam::DQuat::from_axis_angle(right, delta.y * TAU);
        let new_eye = orig.center + r * eye_dir;
        let new_up = r * orig.up;
        self.frame.eye = new_eye;
        self.frame.up = new_up;
        self.last_cam.set(None);
    }

    /// Call when receiving mouse button input.
    pub fn mouse_input(&mut self, button: MouseButton, pressed: bool) {
        match button {
            MouseButton::Middle => {
                if let Some(pos) = self.cursor_pos {
                    match self.input_mode {
                        CameraInputMode::None | CameraInputMode::Pan { .. } if pressed => {
                            self.input_mode = CameraInputMode::Pan {
                                anchor_screen: pos,
                                orig_frame: self.frame,
                            };
                        }
                        CameraInputMode::Pan { orig_frame, anchor_screen } if !pressed => {
                            self.handle_pan(&orig_frame, pos - anchor_screen);
                            self.input_mode = CameraInputMode::None;
                        }
                        _ => {}
                    }
                }
            }
            MouseButton::Left => {
                if let Some(pos) = self.cursor_pos {
                    match self.input_mode {
                        CameraInputMode::None | CameraInputMode::Tumble { .. } if pressed => {
                            self.input_mode = CameraInputMode::Tumble {
                                anchor_screen: pos,
                                orig_frame: self.frame,
                            };
                        }
                        CameraInputMode::Tumble { orig_frame, anchor_screen } if !pressed => {
                            self.handle_tumble(&orig_frame, anchor_screen, pos);
                            self.input_mode = CameraInputMode::None;
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                // TODO
            }
        }
    }

    pub fn mouse_wheel(&mut self, delta: f64) {
        /*
        if (_projType == CameraProjectionType::Perspective) {
                // move camera forwards/backwards, but keep center
                // FIXME this sort of assumes that delta is always 120 or -120
                double deltaF = -0.1 * ((double) delta / 120.0);
                _currentFrame.eye = _currentFrame.center + (1.0 + deltaF) * (_currentFrame.eye - _currentFrame.center);
                updateCamera();
            } else if (_projType == CameraProjectionType::Orthographic) {
                double deltaF = (1.0 - 0.25 * ((double) delta / 120.0));
                const auto height = _camera.GetVerticalAperture();
                const auto aspectRatio = _camera.GetAspectRatio();
                _camera.SetHorizontalAperture(height * deltaF *  aspectRatio);
                _camera.SetVerticalAperture(height * deltaF);
                updateCamera();
            }
        */

        // TODO orthographic projection
        let delta = -0.1 * delta / 120.0;
        self.frame.eye = self.frame.center + (1.0 + delta) * (self.frame.eye - self.frame.center);
        self.last_cam.set(None);
    }

    /// Call when receiving cursor events
    pub fn cursor_moved(&mut self, position: DVec2) {
        self.cursor_pos = Some(position);
        match self.input_mode {
            CameraInputMode::Tumble { orig_frame, anchor_screen } => {
                self.handle_tumble(&orig_frame, anchor_screen, position);
            }
            CameraInputMode::Pan { orig_frame, anchor_screen } => {
                self.handle_pan(&orig_frame, position - anchor_screen);
            }
            _ => {}
        }
    }

    /// Centers the camera on the given axis-aligned bounding box.
    /// Orbit angles are reset.
    pub fn center_on_bounds(&mut self, bounds: &AABB, fov_y_radians: f64) {
        let size = bounds.size().max_element() as f64;
        let new_center: DVec3 = bounds.center().as_dvec3();
        let cam_dist = (0.5 * size) / f64::tan(0.5 * fov_y_radians);

        let new_front = glam::dvec3(0.0, 0.0, -1.0).normalize();
        let new_eye = new_center + (-new_front * cam_dist);

        let new_right = new_front.cross(self.frame.up);
        let new_up = new_right.cross(new_front);

        self.frame.center = new_center;
        self.frame.eye = new_eye;
        self.frame.up = new_up;

        self.z_near = 0.1 * cam_dist;
        self.z_far = 10.0 * cam_dist;
        self.fov_y_radians = fov_y_radians;
        self.last_cam.set(None);

        debug!(
            "center_on_bounds: eye={}, center={}, z_near={}, z_far={}",
            self.frame.eye, self.frame.center, self.z_near, self.z_far
        );
    }

    /// Returns the look-at matrix
    fn get_look_at(&self) -> Mat4 {
        Mat4::look_at_rh(self.frame.eye.as_vec3(), self.frame.center.as_vec3(), self.frame.up.as_vec3())
    }

    /// Returns a `Camera` for the current viewpoint.
    pub fn camera(&self) -> Camera {
        if let Some(cam) = self.last_cam.get() {
            return cam;
        }
        let aspect_ratio = self.screen_size.x / self.screen_size.y;
        let view = self.get_look_at();
        let view_inverse = view.inverse();
        let projection = Mat4::perspective_rh(
            self.fov_y_radians as f32,
            aspect_ratio as f32,
            self.z_near as f32,
            self.z_far as f32,
        );
        let projection_inverse = projection.inverse();
        let cam = Camera {
            frustum: Default::default(),        //TODO
            view,
            view_inverse,
            projection,
            projection_inverse,
            screen_size: self.screen_size,
        };
        self.last_cam.set(Some(cam));
        cam
    }
}
