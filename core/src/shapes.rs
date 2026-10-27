//! Geometric shape strokes. Shapes are ordinary curves, which keeps selection,
//! extrusion, grouping and export uniform.

use crate::brush::Brush;
use crate::curve::{ControlPoint, Curve, CurveKind};
use crate::math::perpendicular;
use crate::{Vec2, Vec3};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeTool {
    Line,
    Circle,
    Ellipse,
    Smooth,
}

impl ShapeTool {
    pub fn label(self) -> &'static str {
        match self {
            ShapeTool::Line => "Line",
            ShapeTool::Circle => "Circle",
            ShapeTool::Ellipse => "Ellipse",
            ShapeTool::Smooth => "Smooth curve",
        }
    }
}

/// Snap a direction to 45° increments in the drawing plane.
pub fn constrain_direction(dir: Vec3, normal: Vec3, step_deg: f32) -> Vec3 {
    let n = normal.normalize_or_zero();
    let u = perpendicular(n);
    let v = n.cross(u).normalize_or_zero();
    let x = dir.dot(u);
    let y = dir.dot(v);
    let len = (x * x + y * y).sqrt();
    if len < 1.0e-6 {
        return dir;
    }
    let angle = y.atan2(x);
    let step = step_deg.to_radians().max(0.001);
    let snapped = (angle / step).round() * step;
    (u * snapped.cos() + v * snapped.sin()) * len
}

/// Create a line. `constrain` snaps to 45° increments in the plane.
pub fn make_line(
    id: crate::CurveId,
    start: Vec3,
    end: Vec3,
    normal: Vec3,
    brush: &Brush,
    constrain: bool,
) -> Curve {
    let end = if constrain {
        start + constrain_direction(end - start, normal, 45.0)
    } else {
        end
    };
    let mut c = Curve::line(id, start, end, brush.clone());
    if (end - start).length() < 1.0e-4 {
        c.points.push(ControlPoint::new(start + Vec3::X * 0.1, 0.8));
    }
    c
}

pub fn make_circle(
    id: crate::CurveId,
    centre: Vec3,
    edge: Vec3,
    normal: Vec3,
    brush: &Brush,
) -> Curve {
    let radius = (edge - centre).length().max(0.25);
    Curve::circle(id, centre, radius, normal, brush.clone())
}

pub fn make_ellipse(
    id: crate::CurveId,
    centre: Vec3,
    radius_x: f32,
    radius_y: f32,
    rotation_deg: f32,
    normal: Vec3,
    brush: &Brush,
) -> Curve {
    let mut c = Curve::ellipse(
        id,
        centre,
        radius_x.max(0.1),
        radius_y.max(0.1),
        rotation_deg.to_radians(),
        brush.clone(),
    );
    // Orient the XY-generated ellipse to the drawing plane.
    let rot = crate::math::rotation_between(Vec3::Z, normal);
    for p in &mut c.points {
        p.position = centre + rot * (p.position - centre);
    }
    c.kind = CurveKind::Ellipse;
    c.refresh_frames();
    c
}

pub fn make_smooth(
    id: crate::CurveId,
    points: Vec<Vec3>,
    pressure: f32,
    brush: &Brush,
    closed: bool,
) -> Curve {
    let cps = points
        .into_iter()
        .map(|p| ControlPoint::new(p, pressure))
        .collect();
    let mut c = Curve::new(id, cps, brush.clone());
    c.kind = CurveKind::CatmullRom;
    c.closed = closed;
    c.name = "Smooth curve".into();
    c.tag = Some("smooth".into());
    c.refresh_frames();
    c
}

/// Create an ellipse from a drag from `centre` to `edge`; the second radius is
/// a fraction of the first. `rotation` follows the drag direction.
pub fn make_ellipse_drag(
    id: crate::CurveId,
    centre: Vec3,
    edge: Vec3,
    normal: Vec3,
    brush: &Brush,
    squash: f32,
) -> Curve {
    let radius = (edge - centre).length().max(0.25);
    let u = perpendicular(normal);
    let v = normal.cross(u).normalize_or_zero();
    let d = edge - centre;
    let rotation = d.dot(v).atan2(d.dot(u)).to_degrees();
    make_ellipse(
        id,
        centre,
        radius,
        radius * squash.clamp(0.05, 1.0),
        rotation,
        normal,
        brush,
    )
}

/// Regular polygon as a closed curve.
pub fn make_polygon(
    id: crate::CurveId,
    centre: Vec3,
    radius: f32,
    sides: u32,
    normal: Vec3,
    rotation: f32,
    brush: &Brush,
) -> Curve {
    let n = sides.clamp(3, 64);
    let u = perpendicular(normal);
    let v = normal.cross(u).normalize_or_zero();
    let points: Vec<Vec3> = (0..n)
        .map(|i| {
            let a = rotation + i as f32 / n as f32 * TAU;
            centre + (u * a.cos() + v * a.sin()) * radius
        })
        .collect();
    let cps = points
        .into_iter()
        .map(|p| ControlPoint::new(p, 0.85))
        .collect();
    let mut c = Curve::new(id, cps, brush.clone());
    c.closed = true;
    c.kind = CurveKind::Polygon;
    c.name = format!("{n}-gon");
    c.tag = Some("polygon".into());
    c.refresh_frames();
    c
}

/// Approximate drag end as world point from a 2D delta along camera axes.
pub fn drag_world_delta(camera: &crate::viewport::Camera, delta_px: Vec2, viewport: Vec2) -> Vec3 {
    let scale =
        2.0 * camera.distance() * (camera.fov_y_deg.to_radians() * 0.5).tan() / viewport.y.max(1.0);
    let (right, up, _) = camera.basis();
    right * (delta_px.x * scale) - up * (delta_px.y * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::CurveKind;

    fn b() -> Brush {
        Brush::default()
    }

    #[test]
    fn circle_radius_is_exact() {
        let c = make_circle(1, Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0), Vec3::Y, &b());
        for s in c.sample(0.5).samples {
            let r = Vec3::new(s.position.x, s.position.y, s.position.z).length();
            assert!((r - 5.0).abs() / 5.0 < 0.01);
        }
    }

    #[test]
    fn line_at_45_degrees() {
        let c = make_line(
            2,
            Vec3::ZERO,
            Vec3::new(10.0, 0.0, 10.0),
            Vec3::Y,
            &b(),
            true,
        );
        let d = c.points[1].position - c.points[0].position;
        assert!((d.y).abs() < 1e-5);
        assert!((d.x.abs() - d.z.abs()).abs() < 1e-4);
    }

    #[test]
    fn constrained_line_snaps() {
        let c = make_line(
            3,
            Vec3::ZERO,
            Vec3::new(10.0, 0.0, 1.0),
            Vec3::Y,
            &b(),
            true,
        );
        let d = c.points[1].position;
        // Snaps to the nearest 45° direction, i.e. horizontal in XZ.
        assert!(d.z.abs() < 1e-3 || (d.x.abs() - d.z.abs()).abs() < 1e-3);
    }

    #[test]
    fn ellipse_has_two_radii() {
        let c = make_ellipse(4, Vec3::ZERO, 8.0, 3.0, 0.0, Vec3::Y, &b());
        let xs = c
            .points
            .iter()
            .map(|p| p.position.x.abs())
            .fold(0.0, f32::max);
        let zs = c
            .points
            .iter()
            .map(|p| p.position.z.abs())
            .fold(0.0, f32::max);
        // The XY ellipse is rotated into the XZ plane.
        assert!((xs - 8.0).abs() < 0.5);
        assert!((zs - 3.0).abs() < 0.5);
    }

    #[test]
    fn polygon_is_closed() {
        let c = make_polygon(5, Vec3::ZERO, 4.0, 6, Vec3::Y, 0.0, &b());
        assert!(c.closed);
        assert_eq!(c.points.len(), 6);
    }

    #[test]
    fn smooth_curve_passes_through_points() {
        let c = make_smooth(
            6,
            vec![Vec3::ZERO, Vec3::X * 5.0, Vec3::new(8.0, 0.0, 5.0)],
            0.8,
            &b(),
            false,
        );
        assert_eq!(c.kind, CurveKind::CatmullRom);
        let s = c.sample(1.0);
        assert!(s.samples.first().unwrap().position.distance(Vec3::ZERO) < 1e-4);
    }

    #[test]
    fn drag_delta_is_in_view_plane() {
        let cam = crate::viewport::Camera::default();
        let d = drag_world_delta(&cam, Vec2::new(100.0, 0.0), Vec2::new(1000.0, 800.0));
        assert!((d.dot(cam.forward())).abs() < 1e-3);
    }
}
