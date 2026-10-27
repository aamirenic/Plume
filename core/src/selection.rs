//! Viewport picking: curves, guides, marquee and lasso selection.

use crate::curve::Curve;
use crate::guide::Guide;
use crate::scene::Scene;
use crate::viewport::{Camera, Ray};
use crate::{CurveId, GuideId, Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct PickResult {
    pub curve: Option<CurveId>,
    pub guide: Option<GuideId>,
    pub curve_distance: f32,
    pub guide_distance: f32,
}

impl PickResult {
    pub fn hit(&self) -> Option<CurveId> {
        self.curve
    }
}

/// Screenshot-space selection settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickSettings {
    /// Screen-space tolerance in points.
    pub tolerance: f32,
    /// Include hidden curves (used by exporters, not by the UI).
    pub include_hidden: bool,
    /// Include locked curves (locked curves cannot be edited but may be
    /// highlighted when Alt is held).
    pub include_locked: bool,
    /// Prefer whichever guide is under the cursor before curves.
    pub guides_first: bool,
}

impl Default for PickSettings {
    fn default() -> Self {
        Self {
            tolerance: 9.0,
            include_hidden: false,
            include_locked: false,
            guides_first: false,
        }
    }
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1.0e-8 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Minimum screen distance from a point to a curve's sampled polyline.
pub fn screen_distance_to_curve(
    curve: &Curve,
    camera: &Camera,
    viewport: Vec2,
    screen: Vec2,
) -> f32 {
    if viewport.x <= 1.0 || viewport.y <= 1.0 {
        return f32::INFINITY;
    }
    let spacing = (curve.brush.base_size * 0.35).max(1.0);
    let points: Vec<Vec2> = curve
        .sample(spacing)
        .samples
        .iter()
        .filter_map(|s| camera.project(s.position, viewport))
        .collect();
    if points.is_empty() {
        return f32::INFINITY;
    }
    if points.len() == 1 {
        return screen.distance(points[0]);
    }
    points
        .windows(2)
        .map(|w| distance_to_segment(screen, w[0], w[1]))
        .fold(f32::INFINITY, f32::min)
}

/// Pick the closest visible curve and, optionally, guide under the cursor.
pub fn pick(
    scene: &Scene,
    camera: &Camera,
    viewport: Vec2,
    screen: Vec2,
    settings: &PickSettings,
) -> PickResult {
    let mut result = PickResult {
        curve_distance: f32::INFINITY,
        guide_distance: f32::INFINITY,
        ..PickResult::default()
    };
    // Guide picking is intentionally separate: it reports where the drawing
    // plane is, not what should become selected after a marquee.  Every
    // visible guide can be picked; the active guide only controls drawing.
    let ray = camera.screen_ray(screen, viewport);
    let mut best_guide = None;
    let mut best_distance = f32::INFINITY;
    for guide in &scene.guides {
        if !guide.visible || guide.locked {
            continue;
        }
        if let Some(hit) = ray_guide(guide, &ray) {
            if hit.distance < best_distance {
                best_distance = hit.distance;
                best_guide = Some(guide.id);
            }
        }
    }
    result.guide = best_guide;
    result.guide_distance = best_distance;

    // Alt-click (guides_first) selects the surface in front of the drawing.
    // Suppressing the curve hit here is what makes that precedence observable
    // to callers, which otherwise always inspect `curve` first.
    let prefer_guide = settings.guides_first && result.guide.is_some();
    if !prefer_guide {
        for curve in &scene.curves {
            if !settings.include_hidden && !scene.curve_visible(curve) {
                continue;
            }
            if !settings.include_locked && scene.curve_locked(curve) {
                continue;
            }
            let d = screen_distance_to_curve(curve, camera, viewport, screen);
            if d <= settings.tolerance && d < result.curve_distance {
                result.curve = Some(curve.id);
                result.curve_distance = d;
            }
        }
    }
    result
}

/// Cast against one guide without rebuilding its BVH when possible. This is a
/// read-only helper for previews; the mutable guide raycast is used for edits.
pub fn ray_guide(guide: &Guide, ray: &Ray) -> Option<crate::guide::GuideHit> {
    // Guide::raycast mutates lazy caches, so a clone keeps the scene immutable.
    // Clones are cheap for primitives and the BVH is rebuilt only if the guide
    // has not yet been built.
    let mut copy = guide.clone();
    copy.raycast(ray)
}

/// Curves whose polyline intersects a screen-space rectangle.
pub fn marquee(
    scene: &Scene,
    camera: &Camera,
    viewport: Vec2,
    min: Vec2,
    max: Vec2,
) -> Vec<CurveId> {
    let min = min.min(max);
    let max = min.max(max);
    let mut out = Vec::new();
    for curve in &scene.curves {
        if !scene.curve_visible(curve) || scene.curve_locked(curve) {
            continue;
        }
        let points: Vec<Vec2> = curve
            .sample((curve.brush.base_size * 0.5).max(1.0))
            .samples
            .iter()
            .filter_map(|s| camera.project(s.position, viewport))
            .collect();
        if points.is_empty() {
            continue;
        }
        let inside = points
            .iter()
            .any(|p| p.cmpge(min).all() && p.cmple(max).all());
        let crosses = points
            .windows(2)
            .any(|w| segment_intersects_rect(w[0], w[1], min, max));
        if inside || crosses {
            out.push(curve.id);
        }
    }
    out
}

fn segment_intersects_rect(a: Vec2, b: Vec2, min: Vec2, max: Vec2) -> bool {
    if (a.x < min.x && b.x < min.x)
        || (a.x > max.x && b.x > max.x)
        || (a.y < min.y && b.y < min.y)
        || (a.y > max.y && b.y > max.y)
    {
        return false;
    }
    // Segment against the four rectangle edges.
    let edges = [
        (Vec2::new(min.x, min.y), Vec2::new(max.x, min.y)),
        (Vec2::new(max.x, min.y), Vec2::new(max.x, max.y)),
        (Vec2::new(max.x, max.y), Vec2::new(min.x, max.y)),
        (Vec2::new(min.x, max.y), Vec2::new(min.x, min.y)),
    ];
    edges.iter().any(|(c, d)| segments_intersect(a, b, *c, *d))
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let orient = |p: Vec2, q: Vec2, r: Vec2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    (o1 * o2 <= 0.0) && (o3 * o4 <= 0.0)
}

/// Lasso selection with a screen-space polygon.
pub fn lasso(scene: &Scene, camera: &Camera, viewport: Vec2, polygon: &[Vec2]) -> Vec<CurveId> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for curve in &scene.curves {
        if !scene.curve_visible(curve) || scene.curve_locked(curve) {
            continue;
        }
        let points: Vec<Vec2> = curve
            .sample((curve.brush.base_size * 0.5).max(1.0))
            .samples
            .iter()
            .filter_map(|s| camera.project(s.position, viewport))
            .collect();
        if points.is_empty() {
            continue;
        }
        let inside = points.iter().any(|p| point_in_polygon(*p, polygon));
        let crosses = points.windows(2).any(|w| {
            polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .take(polygon.len())
                .any(|(a, b)| segments_intersect(w[0], w[1], *a, *b))
        });
        if inside || crosses {
            out.push(curve.id);
        }
    }
    out
}

pub fn point_in_polygon(p: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = polygon.len().saturating_sub(1);
    for i in 0..polygon.len() {
        let pi = polygon[i];
        let pj = polygon[j];
        if (pi.y > p.y) != (pj.y > p.y) {
            let x = (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y).max(-1e12) + pi.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Screen position of the control point closest to `screen`, if any.
pub fn nearest_control_point(
    curve: &Curve,
    camera: &Camera,
    viewport: Vec2,
    screen: Vec2,
    tolerance: f32,
) -> Option<(usize, f32)> {
    curve
        .points
        .iter()
        .enumerate()
        .filter_map(|(index, p)| {
            camera
                .project(p.position, viewport)
                .map(|s| (index, s.distance(screen)))
        })
        .filter(|(_, d)| *d <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Utility used by the guide tool: find the nearest guide surface point.
pub fn guide_surface_point(
    scene: &Scene,
    camera: &Camera,
    viewport: Vec2,
    screen: Vec2,
) -> Option<(GuideId, Vec3, Vec3)> {
    let ray = camera.screen_ray(screen, viewport);
    let mut best: Option<(GuideId, Vec3, Vec3, f32)> = None;
    for guide in &scene.guides {
        if !guide.visible {
            continue;
        }
        if let Some(hit) = ray_guide(guide, &ray) {
            if best.map(|b| hit.distance < b.3).unwrap_or(true) {
                best = Some((guide.id, hit.point, hit.normal, hit.distance));
            }
        }
    }
    best.map(|(id, p, n, _)| (id, p, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;
    use crate::guide::Guide;

    fn camera() -> Camera {
        Camera::new(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 45.0)
    }

    fn scene_with_line() -> (Scene, CurveId) {
        let mut scene = Scene::new();
        let id = scene.new_curve_id();
        let c = Curve::line(
            id,
            Vec3::new(-50.0, 0.0, 0.0),
            Vec3::new(50.0, 0.0, 0.0),
            Brush::default(),
        );
        scene.add_curve(c);
        (scene, id)
    }

    #[test]
    fn picks_a_visible_curve() {
        let (scene, id) = scene_with_line();
        let cam = camera();
        let viewport = Vec2::new(800.0, 600.0);
        let expect = cam.project(Vec3::ZERO, viewport).unwrap();
        let result = pick(&scene, &cam, viewport, expect, &PickSettings::default());
        assert_eq!(result.curve, Some(id));
    }

    #[test]
    fn guides_first_suppresses_the_curve_hit() {
        let (mut scene, _) = scene_with_line();
        let guide = scene.add_guide(Guide::plane(0, Vec3::ZERO, Vec3::Z));
        let cam = camera();
        let viewport = Vec2::new(800.0, 600.0);
        let centre = cam.project(Vec3::ZERO, viewport).unwrap();

        let normal = pick(&scene, &cam, viewport, centre, &PickSettings::default());
        assert!(normal.curve.is_some());
        assert_eq!(normal.guide, Some(guide));

        let preferred = pick(
            &scene,
            &cam,
            viewport,
            centre,
            &PickSettings {
                guides_first: true,
                ..PickSettings::default()
            },
        );
        assert!(preferred.curve.is_none());
        assert_eq!(preferred.guide, Some(guide));
    }

    #[test]
    fn misses_far_away() {
        let (scene, _) = scene_with_line();
        let cam = camera();
        let result = pick(
            &scene,
            &cam,
            Vec2::new(800.0, 600.0),
            Vec2::new(5.0, 5.0),
            &PickSettings::default(),
        );
        assert!(result.curve.is_none());
    }

    #[test]
    fn marquee_catches_crossing_curve() {
        let (scene, id) = scene_with_line();
        let cam = camera();
        let viewport = Vec2::new(800.0, 600.0);
        let found = marquee(
            &scene,
            &cam,
            viewport,
            Vec2::new(0.0, 0.0),
            Vec2::new(800.0, 600.0),
        );
        assert!(found.contains(&id));
    }

    #[test]
    fn lasso_triangle_selects_centre() {
        let (scene, id) = scene_with_line();
        let cam = camera();
        let viewport = Vec2::new(800.0, 600.0);
        let poly = vec![
            Vec2::new(300.0, 100.0),
            Vec2::new(500.0, 500.0),
            Vec2::new(100.0, 500.0),
        ];
        let found = lasso(&scene, &cam, viewport, &poly);
        assert!(found.contains(&id));
    }

    #[test]
    fn nearest_control_point_works() {
        let (scene, id) = scene_with_line();
        let curve = scene.curve(id).unwrap();
        let cam = camera();
        let viewport = Vec2::new(800.0, 600.0);
        let screen = cam.project(curve.points[0].position, viewport).unwrap();
        let found = nearest_control_point(curve, &cam, viewport, screen, 5.0);
        assert_eq!(found.unwrap().0, 0);
    }

    #[test]
    fn guide_raycast_reports_surface() {
        let mut scene = Scene::new();
        scene.add_guide(Guide::plane(1, Vec3::ZERO, Vec3::Z));
        let cam = camera();
        let point = guide_surface_point(
            &scene,
            &cam,
            Vec2::new(800.0, 600.0),
            Vec2::new(400.0, 300.0),
        );
        assert!(point.is_some());
    }

    #[test]
    fn point_in_polygon_basics() {
        let poly = vec![
            Vec2::ZERO,
            Vec2::X * 10.0,
            Vec2::new(10.0, 10.0),
            Vec2::Y * 10.0,
        ];
        assert!(point_in_polygon(Vec2::new(5.0, 5.0), &poly));
        assert!(!point_in_polygon(Vec2::new(15.0, 5.0), &poly));
    }
}
