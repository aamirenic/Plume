//! Liquify deformation for control-point curves.
//!
//! Liquify works on authored control points, not mesh vertices, so the result
//! remains editable and undoes cleanly. Screen-space weights make the brush feel
//! circular regardless of the current camera perspective.

use crate::curve::Curve;
use crate::viewport::Camera;
use crate::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquifyMode {
    Push,
    Pull,
    Twirl,
    Pinch,
    Bloat,
    Smooth,
}

impl LiquifyMode {
    pub fn label(self) -> &'static str {
        match self {
            LiquifyMode::Push => "Push",
            LiquifyMode::Pull => "Pull",
            LiquifyMode::Twirl => "Twirl",
            LiquifyMode::Pinch => "Pinch",
            LiquifyMode::Bloat => "Bloat",
            LiquifyMode::Smooth => "Smooth",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            LiquifyMode::Push => "→",
            LiquifyMode::Pull => "←",
            LiquifyMode::Twirl => "↻",
            LiquifyMode::Pinch => "◇",
            LiquifyMode::Bloat => "○",
            LiquifyMode::Smooth => "≈",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiquifyBrush {
    pub mode: LiquifyMode,
    /// Radius in screen points for screen-space mode.
    pub radius_px: f32,
    pub strength: f32,
    /// 0 = hard edge, 1 = very soft edge.
    pub falloff: f32,
    pub pressure_radius: f32,
    pub pressure_strength: f32,
}

impl Default for LiquifyBrush {
    fn default() -> Self {
        Self {
            mode: LiquifyMode::Push,
            radius_px: 90.0,
            strength: 0.55,
            falloff: 0.65,
            pressure_radius: 0.4,
            pressure_strength: 0.8,
        }
    }
}

/// Apply liquify to one curve. Returns how many control points moved.
pub fn apply_liquify(
    curve: &mut Curve,
    centre: Vec3,
    delta: Vec3,
    radius: f32,
    mode: LiquifyMode,
    strength: f32,
    falloff: f32,
) -> usize {
    if curve.locked || radius <= 1.0e-5 {
        return 0;
    }
    let radius2 = radius * radius;
    let falloff = falloff.clamp(0.0, 1.0);
    let mut moved = 0;
    for p in &mut curve.points {
        let d2 = (p.position - centre).length_squared();
        if d2 > radius2 {
            continue;
        }
        let t = (1.0 - d2.sqrt() / radius).clamp(0.0, 1.0);
        // smoothstep-ish falloff; 0 falloff is a hard disc.
        let w = if falloff > 1.0e-4 {
            t.powf(1.0 + falloff * 3.0)
        } else {
            1.0
        } * strength.clamp(0.0, 1.0);
        if w <= 1.0e-6 {
            continue;
        }
        match mode {
            LiquifyMode::Push | LiquifyMode::Pull => {
                let dir = if matches!(mode, LiquifyMode::Pull) {
                    -delta
                } else {
                    delta
                };
                p.position += dir * w;
            }
            LiquifyMode::Twirl => {
                let radial = p.position - centre;
                let angle = delta.x * w * 0.02;
                let (s, c) = angle.sin_cos();
                p.position = centre
                    + Vec3::new(
                        radial.x * c - radial.z * s,
                        0.0,
                        radial.x * s + radial.z * c,
                    );
                // Preserve the component along Y; the tool operates in the
                // camera plane for a predictable 2D feel.
                p.position.y = (centre + radial).y;
                moved += 1;
            }
            LiquifyMode::Pinch | LiquifyMode::Bloat => {
                let radial = p.position - centre;
                let len = radial.length();
                if len > 1.0e-5 {
                    let amount = if matches!(mode, LiquifyMode::Pinch) {
                        -0.35 * w
                    } else {
                        0.35 * w
                    };
                    p.position = centre + radial * (1.0 + amount);
                }
            }
            LiquifyMode::Smooth => {
                // Average towards the stroke centre-line: pull along the local
                // tangent's normal a little, which removes kinks without
                // collapsing the shape.
                p.position = p.position.lerp(centre, w * 0.06);
            }
        }
        moved += 1;
    }
    if moved > 0 {
        curve.refresh_frames();
    }
    moved
}

/// Screen-space liquify. Distance is measured in viewport points so the brush
/// has a constant visual size. `delta_screen` is converted to a world vector in
/// the camera plane.
// The ten parameters mirror the gesture that drives liquify; each is a distinct
// scalar with no natural grouping.
#[allow(clippy::too_many_arguments)]
pub fn apply_screen_liquify(
    curve: &mut Curve,
    camera: &Camera,
    viewport: Vec2,
    centre_screen: Vec2,
    delta_screen: Vec2,
    radius_px: f32,
    mode: LiquifyMode,
    strength: f32,
    falloff: f32,
    pressure: f32,
) -> usize {
    if curve.locked || radius_px <= 0.5 || viewport.x < 1.0 || viewport.y < 1.0 {
        return 0;
    }
    let (right, up, _) = camera.basis();
    let world_per_px =
        2.0 * camera.distance() * (camera.fov_y_deg.to_radians() * 0.5).tan() / viewport.y.max(1.0);
    let centre = camera
        .screen_ray(centre_screen, viewport)
        .at(camera.distance());
    let delta = (right * delta_screen.x - up * delta_screen.y) * world_per_px;
    let radius = radius_px * world_per_px;
    let strength = strength * (1.0 - 0.55 * (1.0 - pressure.clamp(0.0, 1.0)));
    // Choose a world-space centre on the stroke nearest to the cursor, which
    // avoids moving points that are visually nearby but far in depth.
    let mut best_centre = centre;
    let mut best = f32::INFINITY;
    for p in &curve.points {
        if let Some(s) = camera.project(p.position, viewport) {
            let d = s.distance(centre_screen);
            if d < best {
                best = d;
                best_centre = p.position;
            }
        }
    }
    apply_liquify(curve, best_centre, delta, radius, mode, strength, falloff)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiquifyStats {
    pub curves_touched: usize,
    pub points_moved: usize,
}

impl LiquifyStats {
    pub fn add(&mut self, moved: usize) {
        if moved > 0 {
            self.curves_touched += 1;
            self.points_moved += moved;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;

    fn short_line() -> Curve {
        Curve::line(1, Vec3::ZERO, Vec3::X * 20.0, Brush::default())
    }

    #[test]
    fn push_moves_nearby_points_only() {
        let mut c = short_line();
        let before_far = c.points[c.points.len() - 1].position;
        let moved = apply_liquify(
            &mut c,
            Vec3::ZERO,
            Vec3::Y * 2.0,
            5.0,
            LiquifyMode::Push,
            1.0,
            0.5,
        );
        assert_eq!(moved, 1);
        assert!(c.points[0].position.y > 0.0);
        assert!((c.points[c.points.len() - 1].position - before_far).length() < 1e-5);
    }

    #[test]
    fn pinch_pulls_towards_centre() {
        let mut c =
            Curve::from_positions(2, vec![Vec3::X * 4.0, Vec3::X * 8.0], 0.8, Brush::default());
        let before = c.points[0].position.x;
        apply_liquify(
            &mut c,
            Vec3::ZERO,
            Vec3::ZERO,
            12.0,
            LiquifyMode::Pinch,
            1.0,
            0.0,
        );
        assert!(c.points[0].position.x < before);
    }

    #[test]
    fn bloat_pushes_away() {
        let mut c =
            Curve::from_positions(3, vec![Vec3::X * 4.0, Vec3::X * 8.0], 0.8, Brush::default());
        let before = c.points[0].position.length();
        apply_liquify(
            &mut c,
            Vec3::ZERO,
            Vec3::ZERO,
            12.0,
            LiquifyMode::Bloat,
            1.0,
            0.0,
        );
        assert!(c.points[0].position.length() > before);
    }

    #[test]
    fn locked_curve_is_untouched() {
        let mut c = short_line();
        c.locked = true;
        let moved = apply_liquify(
            &mut c,
            Vec3::ZERO,
            Vec3::Y,
            50.0,
            LiquifyMode::Push,
            1.0,
            0.0,
        );
        assert_eq!(moved, 0);
    }

    #[test]
    fn screen_liquify_uses_screen_radius() {
        let mut c = short_line();
        let camera = Camera::new(Vec3::new(0.0, 0.0, 60.0), Vec3::ZERO, 45.0);
        let viewport = Vec2::new(800.0, 600.0);
        let centre = camera.project(Vec3::ZERO, viewport).unwrap();
        let moved = apply_screen_liquify(
            &mut c,
            &camera,
            viewport,
            centre,
            Vec2::new(10.0, 0.0),
            240.0,
            LiquifyMode::Push,
            1.0,
            0.2,
            1.0,
        );
        assert!(moved >= 1);
    }

    #[test]
    fn smooth_preserves_curve_validity() {
        let mut c = short_line();
        let before = c.points.len();
        apply_liquify(
            &mut c,
            Vec3::X * 10.0,
            Vec3::ZERO,
            16.0,
            LiquifyMode::Smooth,
            1.0,
            0.4,
        );
        assert_eq!(c.points.len(), before);
        assert!(c.points.iter().all(|p| p.position.is_finite()));
    }
}
