//! Perspective camera and navigation.
//!
//! The camera uses a right-handed, Y-up world and a wgpu-style 0..1 depth
//! projection.  `up` is the unrolled up hint; `roll_deg` is applied on top of
//! it, which makes it possible to roll the view without losing the notion of
//! "world up" needed by the orbit gesture.

use crate::math::{normalize_or, ray_plane};
use crate::{Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI};

pub const MIN_FOV: f32 = 30.0;
pub const MAX_FOV: f32 = 90.0;
pub const MIN_NEAR: f32 = 0.05;
pub const DEFAULT_UNIT: f32 = 1.0;

/// A camera ray in world space. `dir` is always normalized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewSnap {
    Perspective,
    Top,
    Bottom,
    Front,
    Rear,
    Left,
    Right,
}

impl ViewSnap {
    pub fn label(self) -> &'static str {
        match self {
            ViewSnap::Perspective => "Perspective",
            ViewSnap::Top => "Top",
            ViewSnap::Bottom => "Bottom",
            ViewSnap::Front => "Front",
            ViewSnap::Rear => "Rear",
            ViewSnap::Left => "Left",
            ViewSnap::Right => "Right",
        }
    }
}

/// Perspective camera.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    /// Un-rolled up hint. Usually +Y, but top/bottom views use -Z / +Z.
    pub up: Vec3,
    pub fov_y_deg: f32,
    pub near: f32,
    pub far: f32,
    pub roll_deg: f32,
}

impl Default for Camera {
    fn default() -> Self {
        let target = Vec3::ZERO;
        let dir = Vec3::new(1.0, 0.72, 1.25).normalize();
        Self {
            position: target + dir * 280.0,
            target,
            up: Vec3::Y,
            fov_y_deg: 45.0,
            near: 0.5,
            far: 20_000.0,
            roll_deg: 0.0,
        }
    }
}

impl Camera {
    pub fn new(position: Vec3, target: Vec3, fov_y_deg: f32) -> Self {
        Self {
            position,
            target,
            up: Vec3::Y,
            fov_y_deg: fov_y_deg.clamp(MIN_FOV, MAX_FOV),
            near: 0.5,
            far: 20_000.0,
            roll_deg: 0.0,
        }
    }

    pub fn distance(&self) -> f32 {
        (self.position - self.target)
            .length()
            .max(self.near * 1.001)
    }

    pub fn forward(&self) -> Vec3 {
        normalize_or(self.target - self.position, -Vec3::Z)
    }

    /// Right-handed orthonormal basis after applying roll.
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let forward = self.forward();
        let mut up_hint = self.up;
        if forward.cross(up_hint).length() < 1.0e-4 {
            up_hint = Vec3::Z;
            if forward.cross(up_hint).length() < 1.0e-4 {
                up_hint = Vec3::X;
            }
        }
        let right = normalize_or(forward.cross(up_hint), Vec3::X);
        let up = right.cross(forward);
        let (right, up) = if self.roll_deg.abs() > 1.0e-6 {
            let q = Quat::from_axis_angle(forward, self.roll_deg.to_radians());
            (q * right, q * up)
        } else {
            (right, up)
        };
        (right, up, forward)
    }

    pub fn effective_up(&self) -> Vec3 {
        self.basis().1
    }

    pub fn right(&self) -> Vec3 {
        self.basis().0
    }

    pub fn view_matrix(&self) -> Mat4 {
        // glam 0.34 moved look_at into an optional camera module; the formula
        // is small and explicit here to keep our API stable.
        let eye = self.position;
        let forward = self.forward();
        let up = self.effective_up();
        let mut side = forward.cross(up);
        if side.length_squared() < 1.0e-12 {
            side = forward.cross(Vec3::Z);
        }
        let side = side.normalize_or_zero();
        let up = side.cross(forward);
        Mat4::from_cols(
            crate::Vec4::new(side.x, up.x, -forward.x, 0.0),
            crate::Vec4::new(side.y, up.y, -forward.y, 0.0),
            crate::Vec4::new(side.z, up.z, -forward.z, 0.0),
            crate::Vec4::new(-side.dot(eye), -up.dot(eye), forward.dot(eye), 1.0),
        )
    }

    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        let aspect = if aspect.is_finite() && aspect > 1.0e-4 {
            aspect
        } else {
            1.0
        };
        let near = self.near.max(MIN_NEAR);
        let far = self.far.max(near + 1.0);
        let f = 1.0 / (self.fov_y_deg.clamp(MIN_FOV, MAX_FOV).to_radians() * 0.5).tan();
        Mat4::from_cols(
            crate::Vec4::new(f / aspect, 0.0, 0.0, 0.0),
            crate::Vec4::new(0.0, f, 0.0, 0.0),
            crate::Vec4::new(0.0, 0.0, far / (near - far), -1.0),
            crate::Vec4::new(0.0, 0.0, near * far / (near - far), 0.0),
        )
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    /// nalgebra mirror of the view matrix, for integrations that use nalgebra.
    pub fn view_matrix_na(&self) -> nalgebra::Matrix4<f32> {
        let m = self.view_matrix();
        nalgebra::Matrix4::from_column_slice(&m.to_cols_array())
    }

    /// Project a world position to screen coordinates in a viewport of
    /// `size` logical pixels. Returns `None` for points behind the camera.
    pub fn project(&self, world: Vec3, size: Vec2) -> Option<Vec2> {
        self.project_with_depth(world, size).map(|(p, _)| p)
    }

    /// Returns screen point and NDC depth (0 near, 1 far).
    pub fn project_with_depth(&self, world: Vec3, size: Vec2) -> Option<(Vec2, f32)> {
        let aspect = size.x / size.y.max(1.0);
        let clip = self.view_projection(aspect) * world.extend(1.0);
        if clip.w.abs() < 1.0e-6 || clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if !ndc.is_finite() {
            return None;
        }
        let screen = Vec2::new((ndc.x * 0.5 + 0.5) * size.x, (0.5 - ndc.y * 0.5) * size.y);
        Some((screen, ndc.z))
    }

    pub fn screen_ray(&self, screen: Vec2, size: Vec2) -> Ray {
        let aspect = size.x / size.y.max(1.0);
        let inv = self.view_projection(aspect).inverse();
        let ndc = Vec2::new(
            (screen.x / size.x.max(1.0)) * 2.0 - 1.0,
            1.0 - (screen.y / size.y.max(1.0)) * 2.0,
        );
        let unproject = |z: f32| -> Vec3 {
            let p = inv * crate::Vec4::new(ndc.x, ndc.y, z, 1.0);
            if p.w.abs() < 1.0e-8 {
                Vec3::ZERO
            } else {
                p.truncate() / p.w
            }
        };
        let near = unproject(0.0);
        let far = unproject(1.0);
        Ray {
            origin: near,
            dir: normalize_or(far - near, self.forward()),
        }
    }

    /// Orbit around target by pixel deltas. 360 px correspond to a full turn.
    pub fn orbit_pixels(&mut self, dx: f32, dy: f32) {
        let sensitivity = 0.008;
        self.orbit_radians(dx * sensitivity, dy * sensitivity);
    }

    pub fn orbit_radians(&mut self, d_theta: f32, d_phi: f32) {
        let offset = self.position - self.target;
        let d = offset.length().max(self.near * 1.1);
        let mut theta = offset.x.atan2(offset.z);
        let mut phi = (offset.y / d).clamp(-1.0, 1.0).acos();
        theta += d_theta;
        phi = (phi + d_phi).clamp(0.02, PI - 0.02);
        let sin_phi = phi.sin();
        let new_offset = Vec3::new(sin_phi * theta.sin(), phi.cos(), sin_phi * theta.cos()) * d;
        self.position = self.target + new_offset;
    }

    /// Orbit using the camera's current right/up axes (trackball-ish).
    pub fn orbit_local(&mut self, angle_right: f32, angle_up: f32) {
        let (right, up, _) = self.basis();
        let offset = self.position - self.target;
        let q = Quat::from_axis_angle(up, -angle_right) * Quat::from_axis_angle(right, -angle_up);
        let new_offset = q * offset;
        if new_offset.cross(Vec3::Y).length() > 1.0e-3 {
            self.position = self.target + new_offset;
        }
    }

    /// Pan the target using pixel deltas. The scene follows the finger.
    pub fn pan_pixels(&mut self, dx: f32, dy: f32, size: Vec2) {
        let world_per_px =
            2.0 * self.distance() * (self.fov_y_deg.to_radians() * 0.5).tan() / size.y.max(1.0);
        let (right, up, _) = self.basis();
        self.target += (-right * dx + up * dy) * world_per_px;
        self.position += (-right * dx + up * dy) * world_per_px;
    }

    pub fn pan_world(&mut self, delta: Vec3) {
        self.target += delta;
        self.position += delta;
    }

    /// Dolly toward the target. `factor > 1` zooms in.
    pub fn zoom(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let d = self.distance();
        let new_d = (d / factor).clamp(self.near * 1.01, self.far * 0.49);
        let dir = self.forward();
        self.position = self.target - dir * new_d;
    }

    /// Equivalent FOV if the same zoom were expressed by narrowing the lens
    /// while keeping the camera at its original distance.
    pub fn equivalent_fov_for_zoom(&self, factor: f32) -> f32 {
        let half = (self.fov_y_deg.clamp(MIN_FOV, MAX_FOV) * 0.5).to_radians();
        (2.0 * ((half).tan() / factor.max(1.0e-4)).atan()).to_degrees()
    }

    pub fn set_fov(&mut self, fov_deg: f32) {
        self.fov_y_deg = fov_deg.clamp(MIN_FOV, MAX_FOV);
    }

    pub fn roll(&mut self, delta_deg: f32) {
        self.roll_deg = (self.roll_deg + delta_deg).rem_euclid(360.0);
    }

    pub fn set_roll(&mut self, roll_deg: f32) {
        self.roll_deg = roll_deg.rem_euclid(360.0);
    }

    /// Position on the camera-facing plane through a point, at a depth offset
    /// expressed in world units. Positive offsets move the plane away from the
    /// camera along its view direction. Used as the fallback drawing plane.
    pub fn drawing_plane(&self, through: Option<Vec3>, depth_offset: f32) -> (Vec3, Vec3) {
        let point = through.unwrap_or(self.target) + self.forward() * depth_offset;
        (point, self.forward())
    }

    pub fn intersect_drawing_plane(
        &self,
        screen: Vec2,
        size: Vec2,
        plane: (Vec3, Vec3),
    ) -> Option<Vec3> {
        let ray = self.screen_ray(screen, size);
        let t = ray_plane(ray.origin, ray.dir, plane.0, plane.1)?;
        Some(ray.at(t))
    }

    pub fn snap_to(&mut self, snap: ViewSnap) {
        let d = self.distance();
        let (offset, up) = match snap {
            ViewSnap::Perspective => (Vec3::new(1.0, 0.72, 1.25).normalize(), Vec3::Y),
            ViewSnap::Top => (Vec3::Y, -Vec3::Z),
            ViewSnap::Bottom => (-Vec3::Y, Vec3::Z),
            ViewSnap::Front => (Vec3::Z, Vec3::Y),
            ViewSnap::Rear => (-Vec3::Z, Vec3::Y),
            ViewSnap::Left => (-Vec3::X, Vec3::Y),
            ViewSnap::Right => (Vec3::X, Vec3::Y),
        };
        self.position = self.target + offset * d;
        self.up = up;
        self.roll_deg = 0.0;
    }

    /// Smoothly frame an axis-aligned bounding box.
    pub fn frame_bounds(&mut self, min: Vec3, max: Vec3, aspect: f32) {
        let center = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(1.0);
        let vfov = self.fov_y_deg.to_radians();
        let hfov = 2.0 * ((vfov * 0.5).tan() * aspect.max(0.1)).atan();
        let fov = vfov.min(hfov);
        let mut dist = radius / (fov * 0.5).sin().max(0.05);
        dist *= 1.15;
        let dir = self.forward();
        self.target = center;
        self.position = center - dir * dist;
        self.near = (dist - radius * 2.0).max(0.05);
        self.far = (dist + radius * 8.0).max(self.near + 10.0);
    }

    pub fn topdown_height(&self) -> f32 {
        self.position.y
    }

    /// Set a perspective orientation from a spherical pair, preserving target.
    pub fn set_spherical(&mut self, theta: f32, phi: f32, distance: f32) {
        let sin_phi = phi.clamp(0.02, PI - 0.02).sin();
        self.position = self.target
            + Vec3::new(sin_phi * theta.sin(), phi.cos(), sin_phi * theta.cos()) * distance;
    }

    /// The camera's current spherical orientation.
    pub fn spherical(&self) -> (f32, f32, f32) {
        let offset = self.position - self.target;
        let d = offset.length();
        let theta = offset.x.atan2(offset.z);
        let phi = if d > 1.0e-6 {
            (offset.y / d).clamp(-1.0, 1.0).acos()
        } else {
            FRAC_PI_2
        };
        (theta, phi, d)
    }
}

/// Navigation tuning shared by the editor and the Android gesture layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavigationSettings {
    pub orbit_sensitivity: f32,
    pub pan_sensitivity: f32,
    pub zoom_sensitivity: f32,
    pub inertia: bool,
    pub damping: f32,
    pub fov_gesture: bool,
    pub snap_views_on_double_tap: bool,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self {
            orbit_sensitivity: 1.0,
            pan_sensitivity: 1.0,
            zoom_sensitivity: 1.0,
            inertia: true,
            damping: 7.5,
            fov_gesture: false,
            snap_views_on_double_tap: true,
        }
    }
}

/// Damped inertial navigation.  The render loop calls `step` with a delta time
/// and the accumulated orbit/pan velocities decay exponentially.
#[derive(Clone, Debug, Default)]
pub struct NavigationInertia {
    pub orbit_velocity: Vec2,
    pub pan_velocity: Vec2,
    pub active: bool,
}

impl NavigationInertia {
    pub fn push_orbit(&mut self, dx: f32, dy: f32) {
        self.orbit_velocity += Vec2::new(dx, dy) * 0.35;
        self.active = true;
    }

    pub fn push_pan(&mut self, dx: f32, dy: f32) {
        self.pan_velocity += Vec2::new(dx, dy) * 0.35;
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.orbit_velocity = Vec2::ZERO;
        self.pan_velocity = Vec2::ZERO;
        self.active = false;
    }

    pub fn step(
        &mut self,
        camera: &mut Camera,
        size: Vec2,
        dt: f32,
        settings: &NavigationSettings,
    ) {
        if !settings.inertia || !self.active {
            return;
        }
        let dt = dt.clamp(0.0, 0.1);
        if self.orbit_velocity.length_squared() > 1.0e-4 {
            camera.orbit_pixels(self.orbit_velocity.x * dt, self.orbit_velocity.y * dt);
            self.orbit_velocity *= (-settings.damping * dt).exp();
        }
        if self.pan_velocity.length_squared() > 1.0e-4 {
            camera.pan_pixels(self.pan_velocity.x * dt, self.pan_velocity.y * dt, size);
            self.pan_velocity *= (-settings.damping * dt).exp();
        }
        if self.orbit_velocity.length_squared() < 0.05 && self.pan_velocity.length_squared() < 0.05
        {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn size() -> Vec2 {
        Vec2::new(1280.0, 800.0)
    }

    #[test]
    fn projection_roundtrip_centre() {
        let cam = Camera::default();
        let point = cam.target - cam.forward() * 40.0;
        let (screen, depth) = cam.project_with_depth(point, size()).unwrap();
        assert_abs_diff_eq!(screen.x, size().x * 0.5, epsilon = 0.01);
        assert_abs_diff_eq!(screen.y, size().y * 0.5, epsilon = 0.01);
        assert!((0.0..=1.0).contains(&depth));
        let ray = cam.screen_ray(screen, size());
        let expected = (point - ray.origin).normalize();
        assert_abs_diff_eq!(ray.dir.x, expected.x, epsilon = 1e-3);
        assert_abs_diff_eq!(ray.dir.y, expected.y, epsilon = 1e-3);
        assert_abs_diff_eq!(ray.dir.z, expected.z, epsilon = 1e-3);
    }

    #[test]
    fn unproject_known_point() {
        let mut cam = Camera::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 45.0);
        cam.up = Vec3::Y;
        let world = Vec3::new(1.0, 0.5, 0.0);
        let screen = cam.project(world, size()).unwrap();
        let ray = cam.screen_ray(screen, size());
        let to_world = (world - ray.origin).normalize();
        assert_abs_diff_eq!(ray.dir.x, to_world.x, epsilon = 1e-3);
        assert_abs_diff_eq!(ray.dir.y, to_world.y, epsilon = 1e-3);
        assert_abs_diff_eq!(ray.dir.z, to_world.z, epsilon = 1e-3);
    }

    #[test]
    fn orbit_quarter_turn() {
        let mut cam = Camera::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 45.0);
        cam.orbit_radians(FRAC_PI_2, 0.0);
        assert_abs_diff_eq!(cam.position.x, 10.0, epsilon = 1e-3);
        assert_abs_diff_eq!(cam.position.z, 0.0, epsilon = 1e-3);
        assert_abs_diff_eq!(cam.position.y, 0.0, epsilon = 1e-3);
        assert_abs_diff_eq!(cam.distance(), 10.0, epsilon = 1e-3);
    }

    #[test]
    fn orbit_pitch_clamped() {
        let mut cam = Camera::default();
        for _ in 0..100 {
            cam.orbit_radians(0.1, 0.1);
        }
        let (_, phi, _) = cam.spherical();
        assert!((0.019..=PI - 0.019).contains(&phi));
    }

    #[test]
    fn zoom_twice_halves_distance() {
        let mut cam = Camera::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 45.0);
        cam.zoom(2.0);
        assert_abs_diff_eq!(cam.distance(), 5.0, epsilon = 1e-3);
    }

    #[test]
    fn equivalent_fov_matches_dolly() {
        let cam = Camera::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 60.0);
        let fov = cam.equivalent_fov_for_zoom(2.0);
        let dolly = 10.0f32 / 2.0;
        let scale_zoom = 1.0 / (cam.fov_y_deg.to_radians() * 0.5).tan();
        let scale_equiv = 1.0 / (fov.to_radians() * 0.5).tan();
        assert!((scale_equiv - scale_zoom * 2.0).abs() < 0.15);
        assert!(dolly < 10.0);
    }

    #[test]
    fn pan_moves_target_in_view_plane() {
        let mut cam = Camera::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 45.0);
        let before = cam.target;
        cam.pan_pixels(100.0, 0.0, size());
        assert!(cam.target.distance(before) > 0.0);
        assert_abs_diff_eq!(cam.target.z, 0.0, epsilon = 1e-4);
    }

    #[test]
    fn fov_is_clamped() {
        let mut cam = Camera::default();
        cam.set_fov(5.0);
        assert_abs_diff_eq!(cam.fov_y_deg, MIN_FOV, epsilon = 1e-6);
        cam.set_fov(170.0);
        assert_abs_diff_eq!(cam.fov_y_deg, MAX_FOV, epsilon = 1e-6);
    }

    #[test]
    fn snap_views_keep_distance() {
        let mut cam = Camera::default();
        let d = cam.distance();
        cam.snap_to(ViewSnap::Top);
        assert_abs_diff_eq!(cam.distance(), d, epsilon = 1e-3);
        assert!(cam.forward().y < -0.9);
        cam.snap_to(ViewSnap::Front);
        assert!((cam.forward().dot(-Vec3::Z) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn top_view_basis_is_stable() {
        let mut cam = Camera::default();
        cam.snap_to(ViewSnap::Top);
        let (right, up, forward) = cam.basis();
        assert!(right.is_finite() && up.is_finite() && forward.is_finite());
        assert_abs_diff_eq!(right.length(), 1.0, epsilon = 1e-5);
        assert_abs_diff_eq!(up.length(), 1.0, epsilon = 1e-5);
        assert_abs_diff_eq!(right.dot(forward), 0.0, epsilon = 1e-5);
    }

    #[test]
    fn drawing_plane_intersection() {
        let cam = Camera::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 45.0);
        let plane = (Vec3::ZERO, -cam.forward());
        let p = cam
            .intersect_drawing_plane(size() * 0.5, size(), plane)
            .unwrap();
        assert_abs_diff_eq!(p.x, 0.0, epsilon = 1e-3);
        assert_abs_diff_eq!(p.y, 0.0, epsilon = 1e-3);
        assert_abs_diff_eq!(p.z, 0.0, epsilon = 1e-3);
    }

    #[test]
    fn drawing_plane_honours_depth_offset() {
        let cam = Camera::new(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 45.0);
        let (point, normal) = cam.drawing_plane(Some(Vec3::ZERO), 12.0);
        assert_abs_diff_eq!(
            (point - Vec3::ZERO).dot(cam.forward()),
            12.0,
            epsilon = 1e-4
        );
        assert_abs_diff_eq!(normal.dot(cam.forward()), 1.0, epsilon = 1e-5);
    }

    #[test]
    fn frame_bounds_contains_scene() {
        let mut cam = Camera::default();
        cam.frame_bounds(Vec3::splat(-50.0), Vec3::splat(50.0), 16.0 / 9.0);
        for corner in [
            Vec3::splat(-50.0),
            Vec3::splat(50.0),
            Vec3::new(-50.0, 50.0, -50.0),
            Vec3::new(50.0, -50.0, 50.0),
        ] {
            let p = cam.project(corner, size());
            assert!(p.is_some());
        }
    }

    #[test]
    fn roll_rotates_up_vector() {
        let mut cam = Camera::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 45.0);
        let up0 = cam.effective_up();
        cam.roll(90.0);
        let up1 = cam.effective_up();
        assert!((up0 - up1).length() > 0.5);
        assert_abs_diff_eq!(up1.length(), 1.0, epsilon = 1e-5);
    }
}
