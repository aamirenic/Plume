//! Stylus capture: smoothing, resampling and surface projection.

use crate::brush::Brush;
use crate::curve::{ControlPoint, Curve};
use crate::guide::{raycast_active, Guide};
use crate::viewport::{Camera, Ray};
use crate::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StylusSample {
    pub screen: Vec2,
    pub pressure: f32,
    /// Degrees from the surface normal.
    pub tilt: f32,
    pub altitude: f32,
    pub azimuth: f32,
    pub barrel_roll: f32,
    pub timestamp: f64,
}

impl Default for StylusSample {
    fn default() -> Self {
        Self {
            screen: Vec2::ZERO,
            pressure: 0.8,
            tilt: 0.0,
            altitude: 90.0,
            azimuth: 0.0,
            barrel_roll: 0.0,
            timestamp: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StrokeConfig {
    /// World spacing between generated control points (millimetres).
    pub spacing: f32,
    /// 0..1 exponential smoothing.
    pub smoothing: f32,
    /// Stable Stroke mode adds lag and stronger smoothing.
    pub stable_stroke: bool,
    pub stable_strength: f32,
    /// Ignore very short strokes when finalising.
    pub minimum_length: f32,
}

impl Default for StrokeConfig {
    fn default() -> Self {
        Self {
            spacing: 2.0,
            smoothing: 0.18,
            stable_stroke: false,
            stable_strength: 0.75,
            minimum_length: 0.4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SurfacePoint {
    pub position: Vec3,
    pub normal: Vec3,
    pub guide_index: Option<usize>,
    pub distance: f32,
}

/// Resolve a screen sample to a 3D point: active guide first, then the
/// camera-facing drawing plane.
pub fn project_to_surface(
    screen: Vec2,
    camera: &Camera,
    viewport: Vec2,
    guides: &mut [Guide],
    drawing_plane: (Vec3, Vec3),
) -> Option<SurfacePoint> {
    let ray = camera.screen_ray(screen, viewport);
    resolve_ray(ray, guides, drawing_plane)
}

pub fn resolve_ray(
    ray: Ray,
    guides: &mut [Guide],
    drawing_plane: (Vec3, Vec3),
) -> Option<SurfacePoint> {
    if let Some(active) = raycast_active(guides, &ray) {
        return Some(SurfacePoint {
            position: active.hit.point,
            normal: active.hit.normal,
            guide_index: Some(active.guide_index),
            distance: active.hit.distance,
        });
    }
    if drawing_plane.1.length_squared() > 1.0e-8 {
        if let Some(t) =
            crate::math::ray_plane(ray.origin, ray.dir, drawing_plane.0, drawing_plane.1)
        {
            return Some(SurfacePoint {
                position: ray.at(t),
                normal: drawing_plane.1.normalize_or_zero(),
                guide_index: None,
                distance: t,
            });
        }
    }
    None
}

/// Smooth a polyline with an exponential moving average. The first point is
/// preserved exactly so endpoints stay under the stylus.
pub fn smooth_points(points: &[Vec3], strength: f32) -> Vec<Vec3> {
    if points.len() < 3 || strength <= 0.0 {
        return points.to_vec();
    }
    let smoothing = strength.clamp(0.0, 1.0);
    let alpha = 1.0 - smoothing * 0.92;
    let mut out = Vec::with_capacity(points.len());
    out.push(points[0]);
    let mut prev = points[0];
    for p in &points[1..] {
        prev += (*p - prev) * alpha;
        out.push(prev);
    }
    if let Some(last) = points.last() {
        if out.len() >= 2 {
            let n = out.len();
            out[n - 1] = *last;
        }
    }
    out
}

/// Resample a polyline to a target spacing. Always keeps both endpoints.
pub fn resample_polyline(points: &[(Vec3, f32)], spacing: f32) -> Vec<(Vec3, f32)> {
    if points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return points.to_vec();
    }
    let spacing = spacing.max(0.05);
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    let mut total = 0.0;
    for w in points.windows(2) {
        total += (w[1].0 - w[0].0).length();
    }
    let mut target = spacing;
    let mut acc = 0.0;
    for w in points.windows(2) {
        let seg_len = (w[1].0 - w[0].0).length();
        if seg_len < 1.0e-9 {
            continue;
        }
        while target <= acc + seg_len && target < total - 1.0e-5 {
            let t = ((target - acc) / seg_len).clamp(0.0, 1.0);
            let p = w[0].0.lerp(w[1].0, t);
            let pressure = w[0].1 * (1.0 - t) + w[1].1 * t;
            out.push((p, pressure));
            target += spacing;
        }
        acc += seg_len;
    }
    let last = *points.last().unwrap();
    if out
        .last()
        .map(|p| p.0.distance(last.0) > 1.0e-5)
        .unwrap_or(true)
    {
        out.push(last);
    }
    out
}

/// Capture raw world samples into a curve. Smoothing is applied before
/// arc-length resampling; both endpoints remain in place.
pub fn build_curve_from_world(
    id: crate::CurveId,
    raw: &[(Vec3, f32, f64)],
    brush: &Brush,
    config: &StrokeConfig,
) -> Option<Curve> {
    if raw.len() < 2 {
        return None;
    }
    let mut points: Vec<Vec3> = raw.iter().map(|(p, _, _)| *p).collect();
    let mut pressures: Vec<f32> = raw.iter().map(|(_, p, _)| p.clamp(0.0, 1.0)).collect();
    let total_length: f32 = points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    if total_length < config.minimum_length {
        return None;
    }
    let smooth_strength = if config.stable_stroke {
        (config.smoothing + config.stable_strength).clamp(0.0, 0.999)
    } else {
        config.smoothing
    };
    points = smooth_points(&points, smooth_strength);
    // Constant-pressure mouse input should not produce synthetic pressure
    // wobble. Normalise obviously flat pressure streams to the brush fallback.
    let pmin = pressures.iter().copied().fold(f32::INFINITY, f32::min);
    let pmax = pressures.iter().copied().fold(0.0f32, f32::max);
    if (pmax - pmin).abs() < 1.0e-4 {
        pressures.fill(brush.stabilization.mouse_pressure);
    }
    let pairs: Vec<(Vec3, f32)> = points
        .iter()
        .copied()
        .zip(pressures.iter().copied())
        .collect();
    let resampled = resample_polyline(&pairs, config.spacing);
    if resampled.len() < 2 {
        return None;
    }
    let control = resampled
        .into_iter()
        .enumerate()
        .map(|(i, (p, pr))| {
            let ts = raw.get(i).map(|r| r.2).unwrap_or(i as f64 * 0.008);
            let mut cp = ControlPoint::new(p, pr);
            cp.timestamp = ts;
            cp
        })
        .collect();
    let curve = Curve::new(id, control, brush.clone());
    Some(curve)
}

/// A closed-shape detector used when the user draws a loop.
pub fn close_if_loop(curve: &mut Curve, max_gap: f32) {
    if curve.points.len() < 8 {
        return;
    }
    let first = curve.points.first().unwrap().position;
    let last = curve.points.last().unwrap().position;
    if first.distance(last) <= max_gap {
        curve.points.pop();
        curve.closed = true;
        curve.kind = crate::curve::CurveKind::Freehand;
        curve.refresh_frames();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::CurveKind;
    use crate::guide::Guide;
    use std::f32::consts::TAU;

    #[test]
    fn straight_line_stays_collinear() {
        let raw: Vec<(Vec3, f32, f64)> = (0..50)
            .map(|i| (Vec3::new(i as f32, 0.0, 0.0), 0.8, i as f64 * 0.01))
            .collect();
        let c =
            build_curve_from_world(1, &raw, &Brush::default(), &StrokeConfig::default()).unwrap();
        assert!(c.points.len() > 10);
        for p in &c.points {
            assert!(p.position.y.abs() < 1e-5);
            assert!(p.position.z.abs() < 1e-5);
        }
    }

    #[test]
    fn circle_stroke_closes_and_stays_on_radius() {
        let n = 400;
        let raw: Vec<(Vec3, f32, f64)> = (0..=n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU;
                (
                    Vec3::new(5.0 * a.cos(), 0.0, 5.0 * a.sin()),
                    0.9,
                    i as f64 * 0.005,
                )
            })
            .collect();
        let mut c =
            build_curve_from_world(2, &raw, &Brush::default(), &StrokeConfig::default()).unwrap();
        close_if_loop(&mut c, 0.5);
        assert!(c.closed);
        assert!(c.kind == CurveKind::Freehand);
        for s in c.sample(0.5).samples {
            let r = Vec3::new(s.position.x, 0.0, s.position.z).length();
            assert!((r - 5.0).abs() / 5.0 < 0.01, "radius {r}");
        }
    }

    #[test]
    fn smoothing_reduces_jitter() {
        let points: Vec<Vec3> = (0..100)
            .map(|i| Vec3::new(i as f32, if i % 2 == 0 { 0.3 } else { -0.3 }, 0.0))
            .collect();
        let smoothed = smooth_points(&points, 0.9);
        let var = |v: &[Vec3]| -> f32 {
            let m = v.iter().map(|p| p.y).sum::<f32>() / v.len() as f32;
            v.iter().map(|p| (p.y - m).powi(2)).sum::<f32>() / v.len() as f32
        };
        assert!(var(&smoothed) < var(&points));
    }

    #[test]
    fn resampling_spacing_is_even() {
        let pts = vec![(Vec3::ZERO, 0.5), (Vec3::X * 10.0, 1.0)];
        let out = resample_polyline(&pts, 2.0);
        for w in out.windows(2) {
            let d = (w[1].0 - w[0].0).length();
            assert!((d - 2.0).abs() < 1e-4);
        }
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn drawing_on_sphere_stays_on_surface() {
        let camera = Camera::default();
        let size = Vec2::new(1000.0, 800.0);
        let mut guides = vec![Guide::sphere(1, Vec3::ZERO, 30.0)];
        guides[0].active = true;
        let plane = (Vec3::ZERO, camera.forward());
        // Fire at the screen centre and a few offsets.
        for dx in [-50.0, 0.0, 50.0] {
            let screen = size * 0.5 + Vec2::new(dx, 0.0);
            if let Some(surface) = project_to_surface(screen, &camera, size, &mut guides, plane) {
                assert_eq!(surface.guide_index, Some(0));
                assert!((surface.position.length() - 30.0).abs() < 0.2);
            }
        }
    }

    #[test]
    fn drawing_plane_used_when_no_guide() {
        let camera = Camera::default();
        let size = Vec2::new(800.0, 800.0);
        let mut guides = vec![];
        let plane = (Vec3::new(0.0, 0.0, 0.0), -camera.forward());
        let p = project_to_surface(size * 0.5, &camera, size, &mut guides, plane).unwrap();
        assert!(p.position.dot(camera.forward()).abs() < 0.5);
        assert_eq!(p.guide_index, None);
    }

    #[test]
    fn too_short_stroke_rejected() {
        let raw = vec![
            (Vec3::ZERO, 0.8, 0.0),
            (Vec3::new(0.01, 0.0, 0.0), 0.8, 0.01),
        ];
        assert!(
            build_curve_from_world(1, &raw, &Brush::default(), &StrokeConfig::default()).is_none()
        );
    }
}
