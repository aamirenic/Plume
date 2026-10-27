//! Small shared math helpers.  Kept free of domain types so every subsystem can
//! use them without cyclic dependencies.

use crate::Vec3;
use std::f32::consts::PI;

pub const EPSILON: f32 = 1.0e-6;
pub const MILLIMETRE: f32 = 1.0;

/// Safe normalization. Degenerate vectors fall back to `fallback` (itself
/// normalized), which keeps frames defined even after a bad edit.
pub fn normalize_or(v: Vec3, fallback: Vec3) -> Vec3 {
    let len = v.length();
    if len.is_finite() && len > EPSILON {
        v / len
    } else {
        let fl = fallback.length();
        if fl > EPSILON {
            fallback / fl
        } else {
            Vec3::Y
        }
    }
}

/// A vector perpendicular to `v`, chosen to be numerically stable.
pub fn perpendicular(v: Vec3) -> Vec3 {
    let v = normalize_or(v, Vec3::Y);
    let a = if v.x.abs() < 0.7 { Vec3::X } else { Vec3::Y };
    normalize_or(v.cross(a), Vec3::Z)
}

/// Shortest-arc rotation taking `from` to `to`. Returns identity for parallel
/// vectors and a stable 180° rotation for antiparallel vectors.
pub fn rotation_between(from: Vec3, to: Vec3) -> crate::Quat {
    let a = normalize_or(from, Vec3::X);
    let b = normalize_or(to, Vec3::X);
    let d = a.dot(b).clamp(-1.0, 1.0);
    if d > 1.0 - 1.0e-6 {
        return crate::Quat::IDENTITY;
    }
    if d < -1.0 + 1.0e-6 {
        let axis = perpendicular(a);
        return crate::Quat::from_axis_angle(axis, PI);
    }
    let axis = a.cross(b);
    crate::Quat::from_axis_angle(axis.normalize(), d.acos())
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp_vec3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a.lerp(b, t)
}

/// Hermite smoothstep. Returns 0 below `edge0`, 1 above `edge1`.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() < EPSILON {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 1 at the centre, smoothly falling to 0 at `radius`.
pub fn falloff_center(radius: f32, distance: f32) -> f32 {
    if radius <= EPSILON {
        return 0.0;
    }
    let t = (distance / radius).clamp(0.0, 1.0);
    let s = 1.0 - t;
    s * s * (3.0 - 2.0 * s)
}

/// Catmull-Rom interpolation (uniform parameterisation) between p1 and p2.
pub fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Cubic Bezier evaluation.
pub fn bezier(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
}

pub fn closest_point_on_segment(p: Vec3, a: Vec3, b: Vec3) -> (Vec3, f32) {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 < EPSILON {
        return (a, 0.0);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (a + ab * t, t)
}

pub fn point_segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    closest_point_on_segment(p, a, b).0.distance(p)
}

pub fn ray_plane(origin: Vec3, dir: Vec3, plane_point: Vec3, plane_normal: Vec3) -> Option<f32> {
    let denom = dir.dot(plane_normal);
    if denom.abs() < 1.0e-6 {
        return None;
    }
    let t = (plane_point - origin).dot(plane_normal) / denom;
    if t >= 0.0 {
        Some(t)
    } else {
        None
    }
}

/// Signed area of a 2D polygon (positive for CCW in a Y-down screen frame).
pub fn polygon_area(points: &[crate::Vec2]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        area += a.x * b.y - b.x * a.y;
    }
    area * 0.5
}

/// Even-odd point in polygon test.
pub fn point_in_polygon(p: crate::Vec2, poly: &[crate::Vec2]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let pi = poly[i];
        let pj = poly[j];
        if ((pi.y > p.y) != (pj.y > p.y))
            && (p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y + 1.0e-12) + pi.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Deterministic 32-bit hash used by procedural patterns and dithering.
pub fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

pub fn hash01(a: u32, b: u32) -> f32 {
    let h = hash_u32(a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b));
    h as f32 / u32::MAX as f32
}

pub fn hash2(x: i32, y: i32) -> f32 {
    hash01(x as u32, y as u32)
}

/// Distance to a line expressed in a 2D pattern cell. Kept in one place so the
/// CPU and WGSL pattern implementations stay conceptually identical.
pub fn line_distance(p: crate::Vec2, angle: f32, spacing: f32) -> f32 {
    let dir = crate::Vec2::new(angle.cos(), angle.sin());
    let normal = crate::Vec2::new(-dir.y, dir.x);
    let d = p.dot(normal);
    let wrapped = (d / spacing.max(1.0e-4)).fract().abs();
    wrapped.min(1.0 - wrapped) * spacing
}

/// Convert sRGB 0..1 to linear. Used by exports and lighting fixtures.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Clamp helper shared by UI and exporters.
pub fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothstep_endpoints() {
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 1.0), 1.0);
        assert!((smoothstep(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn falloff_is_one_at_centre() {
        assert!((falloff_center(2.0, 0.0) - 1.0).abs() < 1e-6);
        assert_eq!(falloff_center(2.0, 2.0), 0.0);
        assert_eq!(falloff_center(0.0, 0.0), 0.0);
    }

    #[test]
    fn catmull_rom_hits_interior_points() {
        let p0 = Vec3::new(-1.0, 0.0, 0.0);
        let p1 = Vec3::ZERO;
        let p2 = Vec3::X;
        let p3 = Vec3::new(2.0, 0.0, 0.0);
        assert!(catmull_rom(p0, p1, p2, p3, 0.0).abs_diff_eq(p1, 1e-5));
        assert!(catmull_rom(p0, p1, p2, p3, 1.0).abs_diff_eq(p2, 1e-5));
    }

    #[test]
    fn rotation_between_axes() {
        let q = rotation_between(Vec3::X, Vec3::Y);
        assert!((q * Vec3::X).abs_diff_eq(Vec3::Y, 1e-5));
        let q = rotation_between(Vec3::X, -Vec3::X);
        assert!((q * Vec3::X).abs_diff_eq(-Vec3::X, 1e-5));
    }

    #[test]
    fn polygon_contains() {
        let poly = vec![
            crate::Vec2::new(0.0, 0.0),
            crate::Vec2::new(10.0, 0.0),
            crate::Vec2::new(10.0, 10.0),
            crate::Vec2::new(0.0, 10.0),
        ];
        assert!(point_in_polygon(crate::Vec2::new(5.0, 5.0), &poly));
        assert!(!point_in_polygon(crate::Vec2::new(15.0, 5.0), &poly));
    }

    #[test]
    fn line_distance_wraps() {
        let d = line_distance(crate::Vec2::new(0.0, 0.1), 0.0, 1.0);
        assert!((d - 0.1).abs() < 1e-5);
        let d = line_distance(crate::Vec2::new(0.0, 0.9), 0.0, 1.0);
        assert!((d - 0.1).abs() < 1e-5);
    }
}
