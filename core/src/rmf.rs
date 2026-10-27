//! Rotation-minimising frames (Bishop frames).
//!
//! Given a polyline we build a frame at every point such that the normal is
//! parallel-transported from the previous point. This avoids the twist and
//! gimbal-lock artefacts of Frenet frames on straight or inflection-heavy
//! strokes. Closed curves get the residual seam twist distributed evenly over
//! the loop.

use crate::math::{normalize_or, perpendicular};
use crate::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RmfFrame {
    pub tangent: Vec3,
    pub normal: Vec3,
    pub binormal: Vec3,
}

impl Default for RmfFrame {
    fn default() -> Self {
        Self {
            tangent: Vec3::Z,
            normal: Vec3::X,
            binormal: Vec3::Y,
        }
    }
}

impl RmfFrame {
    pub fn new(tangent: Vec3, normal: Vec3) -> Self {
        let tangent = normalize_or(tangent, Vec3::Z);
        let normal = normalize_or(
            normal - tangent * normal.dot(tangent),
            perpendicular(tangent),
        );
        let binormal = tangent.cross(normal).normalize_or_zero();
        Self {
            tangent,
            normal,
            binormal,
        }
    }

    pub fn rotate_roll(&self, angle: f32) -> Self {
        if angle.abs() < 1.0e-7 {
            return *self;
        }
        let q = Quat::from_axis_angle(self.tangent, angle);
        Self {
            tangent: self.tangent,
            normal: (q * self.normal).normalize_or_zero(),
            binormal: (q * self.binormal).normalize_or_zero(),
        }
    }

    pub fn is_finite(&self) -> bool {
        self.tangent.is_finite() && self.normal.is_finite() && self.binormal.is_finite()
    }
}

/// Compute tangents with central differences and wrap-around for closed loops.
pub fn compute_tangents(points: &[Vec3], closed: bool) -> Vec<Vec3> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![Vec3::Z];
    }
    let mut t = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = if closed {
            (points[(i + n - 1) % n], points[(i + 1) % n])
        } else if i == 0 {
            (points[0], points[1])
        } else if i == n - 1 {
            (points[n - 2], points[n - 1])
        } else {
            (points[i - 1], points[i + 1])
        };
        let d = b - a;
        if d.length_squared() < 1.0e-12 {
            t.push(t.last().copied().unwrap_or(Vec3::Z));
        } else {
            t.push(d.normalize());
        }
    }
    t
}

/// Initial normal. For closed loops we aim inward (towards the centroid), which
/// makes circular tubes and ribbons read naturally; for open strokes we use the
/// radial direction when it is well-defined, otherwise a stable perpendicular.
pub fn initial_normal(points: &[Vec3], closed: bool) -> Vec3 {
    if points.len() < 2 {
        return Vec3::X;
    }
    let centroid = points.iter().copied().sum::<Vec3>() / points.len() as f32;
    let target = if closed {
        centroid - points[0]
    } else {
        points[0] - centroid
    };
    let t = if points.len() >= 2 {
        (points[1] - points[0]).normalize_or_zero()
    } else {
        Vec3::Z
    };
    let candidate = target - t * target.dot(t);
    if candidate.length() > 1.0e-4 {
        candidate.normalize()
    } else if t.dot(Vec3::Y).abs() < 0.95 {
        (Vec3::Y - t * t.y).normalize()
    } else {
        perpendicular(t)
    }
}

/// Signed angle from `a` to `b` around axis `axis`.
pub fn signed_angle(a: Vec3, b: Vec3, axis: Vec3) -> f32 {
    let cross = a.cross(b);
    cross.dot(axis).atan2(a.dot(b))
}

/// Build `RmfFrame`s for a polyline. `points.len()` frames are returned.
pub fn compute_rmf(points: &[Vec3], closed: bool) -> Vec<RmfFrame> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    let tangents = compute_tangents(points, closed);
    if n == 1 {
        return vec![RmfFrame::new(tangents[0], Vec3::X)];
    }
    let mut frames = Vec::with_capacity(n);
    let n0 = initial_normal(points, closed);
    frames.push(RmfFrame::new(tangents[0], n0));
    for i in 1..n {
        let prev = frames[i - 1];
        let q = crate::math::rotation_between(prev.tangent, tangents[i]);
        let mut normal = q * prev.normal;
        normal -= tangents[i] * normal.dot(tangents[i]);
        if normal.length_squared() < 1.0e-10 {
            normal = perpendicular(tangents[i]);
        }
        frames.push(RmfFrame::new(tangents[i], normal));
    }

    if closed && n > 1 {
        // How much does the frame twist when the loop closes?
        let end_rotated =
            crate::math::rotation_between(tangents[n - 1], tangents[0]) * frames[n - 1].normal;
        let seam = signed_angle(frames[0].normal, end_rotated, tangents[0]);
        if seam.is_finite() && seam.abs() > 1.0e-7 {
            for (i, f) in frames.iter_mut().enumerate() {
                let s = i as f32 / n as f32;
                *f = f.rotate_roll(-seam * s);
            }
        }
    }

    frames
}

/// Shortest rotation axis and angle between consecutive tangents (used by
/// tests and by adaptive tessellation).
pub fn turn_angles(points: &[Vec3], closed: bool) -> Vec<f32> {
    let tangents = compute_tangents(points, closed);
    if tangents.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(tangents.len());
    for i in 0..tangents.len() {
        let j = if i + 1 < tangents.len() {
            i + 1
        } else if closed {
            0
        } else {
            break;
        };
        out.push(tangents[i].dot(tangents[j]).clamp(-1.0, 1.0).acos());
    }
    out
}

/// Roll-rotate every frame by an angle (used by barrel-roll brush dynamics).
pub fn apply_roll(frames: &mut [RmfFrame], angle: f32) {
    for f in frames {
        *f = f.rotate_roll(angle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn line(n: usize) -> Vec<Vec3> {
        (0..n)
            .map(|i| Vec3::new(i as f32 * 2.0, 0.0, 0.0))
            .collect()
    }

    fn circle(n: usize, r: f32) -> Vec<Vec3> {
        (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU;
                Vec3::new(r * a.cos(), r * a.sin(), 0.0)
            })
            .collect()
    }

    #[test]
    fn straight_line_frames_identical() {
        let pts = line(20);
        let frames = compute_rmf(&pts, false);
        assert_eq!(frames.len(), pts.len());
        for f in &frames {
            assert!((f.normal - frames[0].normal).length() < 1.0e-4);
            assert!((f.tangent - Vec3::X).length() < 1.0e-4);
        }
    }

    #[test]
    fn frames_are_orthonormal() {
        let pts = circle(64, 5.0);
        for f in compute_rmf(&pts, true) {
            assert!((f.tangent.length() - 1.0).abs() < 1.0e-5);
            assert!((f.normal.length() - 1.0).abs() < 1.0e-5);
            assert!((f.binormal.length() - 1.0).abs() < 1.0e-5);
            assert!(f.tangent.dot(f.normal).abs() < 1.0e-5);
            assert!(f.tangent.dot(f.binormal).abs() < 1.0e-5);
            assert!(f.normal.dot(f.binormal).abs() < 1.0e-5);
        }
    }

    #[test]
    fn circle_normal_points_inside() {
        let pts = circle(96, 4.0);
        for (p, f) in pts.iter().zip(compute_rmf(&pts, true)) {
            let radial = *p;
            assert!(
                f.normal.dot(radial) < -0.9,
                "normal {:?} radial {:?}",
                f.normal,
                radial
            );
        }
    }

    #[test]
    fn closed_seam_is_minimal() {
        let pts = circle(72, 3.0);
        let frames = compute_rmf(&pts, true);
        let end_rot =
            crate::math::rotation_between(frames.last().unwrap().tangent, frames[0].tangent)
                * frames.last().unwrap().normal;
        let seam = signed_angle(frames[0].normal, end_rot, frames[0].tangent);
        // A sixth of a degree or better.
        assert!(seam.abs() < 0.01, "seam twist {seam}");
    }

    #[test]
    fn helix_has_no_ldegenerate_frames() {
        let pts: Vec<Vec3> = (0..200)
            .map(|i| {
                let t = i as f32 * 0.1;
                Vec3::new(t.cos() * 3.0, t.sin() * 3.0, t * 1.5)
            })
            .collect();
        let frames = compute_rmf(&pts, false);
        let mut max_step = 0.0f32;
        for w in frames.windows(2) {
            assert!(w[0].normal.is_finite() && w[1].normal.is_finite());
            let d = w[0].normal.dot(w[1].normal).clamp(-1.0, 1.0).acos();
            max_step = max_step.max(d);
        }
        assert!(max_step < 0.25, "unexpected normal jump {max_step}");
    }

    #[test]
    fn sharp_corner_remains_finite() {
        let pts = vec![
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 1.0),
        ];
        let frames = compute_rmf(&pts, false);
        assert!(frames.iter().all(RmfFrame::is_finite));
    }

    #[test]
    fn barrel_roll_rotates_around_tangent() {
        let f = RmfFrame::new(Vec3::X, Vec3::Y);
        let g = f.rotate_roll(std::f32::consts::FRAC_PI_2);
        assert!((g.normal - Vec3::Z).length() < 1.0e-5);
        assert_eq!(g.tangent, Vec3::X);
    }

    #[test]
    fn degenerate_points_do_not_panic() {
        let pts = vec![Vec3::ZERO; 4];
        let frames = compute_rmf(&pts, false);
        assert_eq!(frames.len(), 4);
        assert!(frames.iter().all(RmfFrame::is_finite));
    }
}
