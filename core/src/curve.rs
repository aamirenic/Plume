//! Curve data model, evaluation and adaptive sampling.
//!
//! A curve owns control points and a brush snapshot.  Geometry is rebuilt on
//! demand; arcs are parameterised by arc length so animation, mapping and
//! extrusion all move at a uniform speed.

use crate::brush::Brush;
use crate::math::{bezier, clamp01};
use crate::rmf::{compute_rmf, RmfFrame};
use crate::{CurveId, GroupId, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlPoint {
    pub position: Vec3,
    /// Handle offsets relative to `position`.
    pub handle_in: Vec3,
    pub handle_out: Vec3,
    pub pressure: f32,
    pub timestamp: f64,
}

impl ControlPoint {
    pub fn new(position: Vec3, pressure: f32) -> Self {
        Self {
            position,
            handle_in: Vec3::ZERO,
            handle_out: Vec3::ZERO,
            pressure: pressure.clamp(0.0, 1.0),
            timestamp: 0.0,
        }
    }

    pub fn with_handles(mut self, handle_in: Vec3, handle_out: Vec3) -> Self {
        self.handle_in = handle_in;
        self.handle_out = handle_out;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveKind {
    Freehand,
    CatmullRom,
    Bezier,
    Line,
    Circle,
    Ellipse,
    Polygon,
}

impl CurveKind {
    pub fn label(self) -> &'static str {
        match self {
            CurveKind::Freehand => "Freehand",
            CurveKind::CatmullRom => "Smooth curve",
            CurveKind::Bezier => "Bezier",
            CurveKind::Line => "Line",
            CurveKind::Circle => "Circle",
            CurveKind::Ellipse => "Ellipse",
            CurveKind::Polygon => "Polygon",
        }
    }

    pub fn is_closed_by_default(self) -> bool {
        matches!(
            self,
            CurveKind::Circle | CurveKind::Ellipse | CurveKind::Polygon
        )
    }
}

/// One sampled point of a curve, ready for extrusion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveSample {
    pub position: Vec3,
    pub normal: Vec3,
    pub binormal: Vec3,
    pub tangent: Vec3,
    pub pressure: f32,
    /// Normalised arc length.
    pub u: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveSamples {
    pub samples: Vec<CurveSample>,
    pub arc_length: f32,
    pub closed: bool,
}

impl CurveSamples {
    pub fn positions(&self) -> Vec<Vec3> {
        self.samples.iter().map(|s| s.position).collect()
    }

    pub fn frames(&self) -> Vec<RmfFrame> {
        self.samples
            .iter()
            .map(|s| RmfFrame {
                tangent: s.tangent,
                normal: s.normal,
                binormal: s.binormal,
            })
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Curve {
    pub id: CurveId,
    pub name: String,
    pub points: Vec<ControlPoint>,
    pub tension: f32,
    pub closed: bool,
    pub brush: Brush,
    pub color: Vec4,
    pub visible: bool,
    pub group: Option<GroupId>,
    pub kind: CurveKind,
    /// Frames are cached in the document so export and file round-trips do not
    /// need to recompute spline geometry.
    #[serde(default)]
    pub frames: Vec<RmfFrame>,
    /// Brushes with patterns can be cut out in 3D export.
    pub cutout: bool,
    /// Optional label such as "circle" or "mirror-x".
    pub tag: Option<String>,
    pub locked: bool,
}

impl Curve {
    pub fn new(id: CurveId, points: Vec<ControlPoint>, brush: Brush) -> Self {
        let kind = CurveKind::Freehand;
        let mut c = Self {
            id,
            name: "Stroke".into(),
            points,
            tension: 0.5,
            closed: false,
            color: brush.color,
            visible: true,
            group: None,
            kind,
            frames: Vec::new(),
            cutout: false,
            tag: None,
            locked: false,
            brush,
        };
        c.refresh_frames();
        c
    }

    pub fn from_positions(id: CurveId, positions: Vec<Vec3>, pressure: f32, brush: Brush) -> Self {
        let points = positions
            .into_iter()
            .map(|p| ControlPoint::new(p, pressure))
            .collect();
        Self::new(id, points, brush)
    }

    pub fn with_kind(mut self, kind: CurveKind) -> Self {
        self.kind = kind;
        if kind.is_closed_by_default() {
            self.closed = true;
        }
        self.refresh_frames();
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn refresh_frames(&mut self) {
        let samples = self.sample(2.0);
        self.frames = samples.frames();
    }

    pub fn bounds(&self) -> (Vec3, Vec3) {
        if self.points.is_empty() {
            return (Vec3::ZERO, Vec3::ZERO);
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.points {
            min = min.min(p.position);
            max = max.max(p.position);
        }
        (min, max)
    }

    pub fn centre(&self) -> Vec3 {
        let (a, b) = self.bounds();
        (a + b) * 0.5
    }

    fn segment_count(&self) -> usize {
        if self.points.len() < 2 {
            0
        } else if self.closed {
            self.points.len()
        } else {
            self.points.len() - 1
        }
    }

    fn point_wrapped(&self, i: isize) -> Vec3 {
        let n = self.points.len() as isize;
        if n == 0 {
            return Vec3::ZERO;
        }
        if self.closed {
            self.points[i.rem_euclid(n) as usize].position
        } else {
            self.points[i.clamp(0, n - 1) as usize].position
        }
    }

    fn pressure_at_index(&self, i: isize) -> f32 {
        let n = self.points.len() as isize;
        if n == 0 {
            return 0.8;
        }
        if self.closed {
            self.points[i.rem_euclid(n) as usize].pressure
        } else {
            self.points[i.clamp(0, n - 1) as usize].pressure
        }
    }

    /// Position on the underlying spline at a segment-local parameter. This is
    /// not arc-length corrected; `eval` handles that.
    fn eval_segment(&self, segment: usize, t: f32) -> (Vec3, f32) {
        let n = self.points.len();
        if n == 0 {
            return (Vec3::ZERO, 0.8);
        }
        if n == 1 {
            return (self.points[0].position, self.points[0].pressure);
        }
        let t = clamp01(t);
        let i = segment as isize;
        match self.kind {
            CurveKind::Line | CurveKind::Polygon => {
                let a = i as usize % n;
                let b = if self.closed {
                    (i as usize + 1) % n
                } else {
                    (i as usize + 1).min(n - 1)
                };
                let p0 = self.points[a].position;
                let p1 = self.points[b].position;
                (
                    p0.lerp(p1, t),
                    self.points[a].pressure * (1.0 - t) + self.points[b].pressure * t,
                )
            }
            CurveKind::Bezier => {
                let a = i as usize % n;
                let b = if self.closed {
                    (i as usize + 1) % n
                } else {
                    (i as usize + 1).min(n - 1)
                };
                let p0 = self.points[a].position;
                let p1 = self.points[a].position + self.points[a].handle_out;
                let p2 = self.points[b].position + self.points[b].handle_in;
                let p3 = self.points[b].position;
                (
                    bezier(p0, p1, p2, p3, t),
                    self.points[a].pressure * (1.0 - t) + self.points[b].pressure * t,
                )
            }
            _ => {
                let p0 = self.point_wrapped(i - 1);
                let p1 = self.point_wrapped(i);
                let p2 = self.point_wrapped(i + 1);
                let p3 = self.point_wrapped(i + 2);
                // Cardinal spline. tension 0.5 == uniform Catmull-Rom.
                let tension = (1.0 - self.tension).clamp(0.0, 1.5);
                let m1 = (p2 - p0) * tension;
                let m2 = (p3 - p1) * tension;
                let t2 = t * t;
                let t3 = t2 * t;
                let p = (2.0 * t3 - 3.0 * t2 + 1.0) * p1
                    + (t3 - 2.0 * t2 + t) * m1
                    + (-2.0 * t3 + 3.0 * t2) * p2
                    + (t3 - t2) * m2;
                let pr1 = self.pressure_at_index(i);
                let pr2 = self.pressure_at_index(i + 1);
                (p, pr1 * (1.0 - t) + pr2 * t)
            }
        }
    }

    /// Fine polyline used for arc-length lookups. 32 subdivisions per segment
    /// keeps a 1 mm curve accurate to well below a tenth of a millimetre.
    fn fine_table(&self, subdivisions: usize) -> (Vec<Vec3>, Vec<f32>, Vec<f32>) {
        let segments = self.segment_count();
        if segments == 0 {
            let p = self
                .points
                .first()
                .map(|p| p.position)
                .unwrap_or(Vec3::ZERO);
            let pr = self.points.first().map(|p| p.pressure).unwrap_or(0.8);
            return (vec![p], vec![pr], vec![0.0]);
        }
        let subdivisions = subdivisions.max(2);
        let mut positions = Vec::with_capacity(segments * subdivisions + 1);
        let mut pressures = Vec::with_capacity(positions.capacity());
        let mut lengths = Vec::with_capacity(positions.capacity());
        let mut last: Option<Vec3> = None;
        for seg in 0..segments {
            for j in 0..subdivisions {
                let t = j as f32 / subdivisions as f32;
                let (p, pr) = self.eval_segment(seg, t);
                if let Some(l) = last {
                    lengths.push((p - l).length());
                } else {
                    lengths.push(0.0);
                }
                positions.push(p);
                pressures.push(pr);
                last = Some(p);
            }
        }
        let (p, pr) = self.eval_segment(segments - 1, 1.0);
        if let Some(l) = last {
            lengths.push((p - l).length());
        }
        positions.push(p);
        pressures.push(pr);
        (positions, pressures, lengths)
    }

    /// Evaluate at arc-length-parameterised `t` in 0..1.
    pub fn eval(&self, t: f32) -> (Vec3, f32) {
        if self.points.is_empty() {
            return (Vec3::ZERO, 0.8);
        }
        if self.points.len() == 1 {
            return (self.points[0].position, self.points[0].pressure);
        }
        let t = clamp01(t);
        let (positions, pressures, seg_lengths) = self.fine_table(24);
        let total: f32 = seg_lengths.iter().sum();
        if total <= 1.0e-6 {
            return (positions[0], pressures[0]);
        }
        if self.closed && t >= 1.0 - 1.0e-6 {
            return (positions[0], pressures[0]);
        }
        let target = t * total;
        let mut acc = 0.0;
        for i in 0..seg_lengths.len().saturating_sub(1) {
            let next = acc + seg_lengths[i + 1];
            if target <= next {
                let local = if seg_lengths[i + 1] > 1.0e-8 {
                    (target - acc) / seg_lengths[i + 1]
                } else {
                    0.0
                };
                let p = positions[i].lerp(positions[i + 1], local);
                let pr = pressures[i] * (1.0 - local) + pressures[i + 1] * local;
                return (p, pr);
            }
            acc = next;
        }
        (*positions.last().unwrap(), *pressures.last().unwrap())
    }

    pub fn eval_position(&self, t: f32) -> Vec3 {
        self.eval(t).0
    }

    pub fn tangent(&self, t: f32) -> Vec3 {
        let e = 0.002;
        let a = self.eval((t - e).max(0.0)).0;
        let b = self.eval((t + e).min(1.0)).0;
        (b - a).normalize_or_zero()
    }

    /// Sample at an approximately uniform spacing (world units).
    pub fn sample(&self, spacing: f32) -> CurveSamples {
        let spacing = spacing.max(0.05);
        if self.points.is_empty() {
            return CurveSamples {
                samples: Vec::new(),
                arc_length: 0.0,
                closed: self.closed,
            };
        }
        if self.points.len() == 1 {
            let p = &self.points[0];
            return CurveSamples {
                samples: vec![CurveSample {
                    position: p.position,
                    normal: Vec3::X,
                    binormal: Vec3::Y,
                    tangent: Vec3::Z,
                    pressure: p.pressure,
                    u: 0.0,
                }],
                arc_length: 0.0,
                closed: self.closed,
            };
        }
        let (positions, pressures, seg_lengths) = self.fine_table(32);
        let total: f32 = seg_lengths.iter().sum();
        if total <= 1.0e-6 {
            let f = compute_rmf(&[positions[0]], false);
            let p = &self.points[0];
            return CurveSamples {
                samples: vec![CurveSample {
                    position: p.position,
                    normal: f[0].normal,
                    binormal: f[0].binormal,
                    tangent: f[0].tangent,
                    pressure: p.pressure,
                    u: 0.0,
                }],
                arc_length: 0.0,
                closed: self.closed,
            };
        }
        let mut cumulative = Vec::with_capacity(positions.len());
        let mut acc = 0.0;
        for l in &seg_lengths {
            acc += *l;
            cumulative.push(acc);
        }
        let mut out_positions = Vec::new();
        let mut out_pressures = Vec::new();
        let mut out_u = Vec::new();
        let steps = (total / spacing).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let target = (k as f32 * spacing).min(total);
            let mut idx = 0usize;
            while idx + 1 < cumulative.len() && cumulative[idx + 1] < target {
                idx += 1;
            }
            let idx = idx.min(cumulative.len().saturating_sub(2));
            let start_len = cumulative[idx];
            let len = (cumulative[idx + 1] - start_len).max(1.0e-8);
            let local = ((target - start_len) / len).clamp(0.0, 1.0);
            let p = positions[idx].lerp(positions[idx + 1], local);
            let pr = pressures[idx] * (1.0 - local) + pressures[idx + 1] * local;
            out_positions.push(p);
            out_pressures.push(pr);
            out_u.push(target / total);
        }
        // For open curves make sure the true endpoint is present.
        if !self.closed {
            if let Some(last) = positions.last() {
                if out_positions
                    .last()
                    .map(|p| p.distance(*last) > 1.0e-4)
                    .unwrap_or(true)
                {
                    out_positions.push(*last);
                    out_pressures.push(*pressures.last().unwrap());
                    out_u.push(1.0);
                }
            }
        } else if out_positions.len() > 1 {
            // No duplicate seam vertex for closed loops.
            if out_positions[0].distance(*out_positions.last().unwrap()) < 1.0e-4 {
                out_positions.pop();
                out_pressures.pop();
                out_u.pop();
            }
        }
        let frames = compute_rmf(&out_positions, self.closed);
        let samples = out_positions
            .iter()
            .zip(out_pressures.iter())
            .zip(out_u.iter())
            .zip(frames.iter())
            .map(|(((&position, &pressure), &u), frame)| CurveSample {
                position,
                normal: frame.normal,
                binormal: frame.binormal,
                tangent: frame.tangent,
                pressure,
                u,
            })
            .collect();
        CurveSamples {
            samples,
            arc_length: total,
            closed: self.closed,
        }
    }

    /// Adaptive sampling: denser near high curvature. Used by exporters where
    /// silhouette quality matters more than exact spacing. `max_spacing` is a
    /// hard upper bound, so callers can request a coarse base while still
    /// guaranteeing a minimum tessellation density.
    pub fn sample_adaptive(&self, base_spacing: f32, max_spacing: f32) -> CurveSamples {
        let cap = max_spacing.max(0.05);
        let base_spacing = base_spacing.clamp(0.05, cap);
        let base = self.sample(base_spacing);
        if base.samples.len() < 3 {
            return base;
        }
        let mut needs_refinement = false;
        for w in base.samples.windows(3) {
            let a = (w[1].position - w[0].position).normalize_or_zero();
            let b = (w[2].position - w[1].position).normalize_or_zero();
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            if angle > 0.35 {
                needs_refinement = true;
                break;
            }
        }
        if !needs_refinement {
            return base;
        }
        // Re-sample the spline with a finer fixed spacing; this is simpler and
        // more stable than splicing vertices after frame construction. Keep the
        // result no coarser than the requested maximum.
        self.sample((base_spacing * 0.5).clamp(0.05, cap))
    }

    pub fn translate(&mut self, delta: Vec3) {
        for p in &mut self.points {
            p.position += delta;
        }
        self.frames.clear();
        self.refresh_frames();
    }

    pub fn transform(&mut self, mat: crate::Mat4) {
        let normal_mat = crate::Mat3::from_mat4(mat).inverse().transpose();
        for p in &mut self.points {
            p.position = mat.transform_point3(p.position);
            p.handle_in = normal_mat * p.handle_in;
            p.handle_out = normal_mat * p.handle_out;
        }
        self.frames.clear();
        self.refresh_frames();
    }

    pub fn reverse(&mut self) {
        self.points.reverse();
        for p in &mut self.points {
            std::mem::swap(&mut p.handle_in, &mut p.handle_out);
        }
        self.frames.clear();
        self.refresh_frames();
    }

    /// Insert a point at the closest location to `position` (point eraser is
    /// the inverse). Returns the index.
    pub fn add_point_at(&mut self, index: usize, point: ControlPoint) {
        let index = index.min(self.points.len());
        self.points.insert(index, point);
        self.frames.clear();
        self.refresh_frames();
    }

    pub fn remove_point(&mut self, index: usize) -> Option<ControlPoint> {
        if index < self.points.len() {
            let p = self.points.remove(index);
            self.frames.clear();
            self.refresh_frames();
            Some(p)
        } else {
            None
        }
    }

    pub fn style(&self) -> CurveStyle {
        CurveStyle {
            color: self.color,
            size: self.brush.base_size,
            opacity: self.brush.opacity,
            pattern: self.brush.pattern.kind,
        }
    }

    /// Recreate a circular stroke. 64 control points keep the Catmull-Rom
    /// error below 0.1% for a normal-sized circle.
    pub fn circle(id: CurveId, centre: Vec3, radius: f32, normal: Vec3, brush: Brush) -> Self {
        let n = 64;
        let (u, v) = {
            let n = crate::math::normalize_or(normal, Vec3::Z);
            let a = crate::math::perpendicular(n);
            (a, n.cross(a).normalize())
        };
        let points = (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU;
                ControlPoint::new(
                    centre + (u * a.cos() + v * a.sin()) * radius,
                    brush.dynamics.shape_pressure(0.8),
                )
            })
            .collect();
        let mut c = Self::new(id, points, brush);
        c.closed = true;
        c.kind = CurveKind::Circle;
        c.name = "Circle".into();
        c.tag = Some("circle".into());
        c.refresh_frames();
        c
    }

    pub fn line(id: CurveId, a: Vec3, b: Vec3, brush: Brush) -> Self {
        let pr = brush.dynamics.shape_pressure(0.9);
        let mut c = Self::new(
            id,
            vec![ControlPoint::new(a, pr), ControlPoint::new(b, pr)],
            brush,
        );
        c.kind = CurveKind::Line;
        c.name = "Line".into();
        c.tag = Some("line".into());
        c
    }

    pub fn ellipse(
        id: CurveId,
        centre: Vec3,
        radius_x: f32,
        radius_y: f32,
        rotation: f32,
        brush: Brush,
    ) -> Self {
        let n = 72;
        let points = (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU;
                let local = Vec2::new(radius_x * a.cos(), radius_y * a.sin());
                let (s, c) = rotation.sin_cos();
                let p = Vec2::new(local.x * c - local.y * s, local.x * s + local.y * c);
                ControlPoint::new(centre + Vec3::new(p.x, p.y, 0.0), 0.8)
            })
            .collect();
        let mut curve = Self::new(id, points, brush);
        curve.closed = true;
        curve.kind = CurveKind::Ellipse;
        curve.name = "Ellipse".into();
        curve.tag = Some("ellipse".into());
        curve.refresh_frames();
        curve
    }

    /// Projected screen-space polyline for selection and picking.
    pub fn screen_points(
        &self,
        camera: &crate::viewport::Camera,
        size: Vec2,
        spacing: f32,
    ) -> Vec<Vec2> {
        self.sample(spacing)
            .samples
            .iter()
            .filter_map(|s| camera.project(s.position, size))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveStyle {
    pub color: Vec4,
    pub size: f32,
    pub opacity: f32,
    pub pattern: crate::pattern::PatternKind,
}

/// Create a mirrored copy of a curve across an arbitrary plane.
pub fn mirror_curve(id: CurveId, curve: &Curve, plane_point: Vec3, plane_normal: Vec3) -> Curve {
    let n = plane_normal.normalize_or_zero();
    let reflect = |p: Vec3| -> Vec3 { p - n * (2.0 * (p - plane_point).dot(n)) };
    let mut out = curve.clone();
    out.id = id;
    out.name = format!("{} mirror", curve.name);
    let reflect_vec = |v: Vec3| -> Vec3 { v - n * (2.0 * v.dot(n)) };
    for p in &mut out.points {
        p.position = reflect(p.position);
        p.handle_in = reflect_vec(p.handle_in);
        p.handle_out = reflect_vec(p.handle_out);
    }
    // Reflection reverses winding; reverse the point order to preserve normals.
    out.reverse();
    out.tag = Some("mirror".into());
    out.refresh_frames();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::normalize_or;
    use std::f32::consts::TAU;

    fn b() -> Brush {
        Brush::default()
    }

    #[test]
    fn straight_line_samples_are_collinear() {
        let mut c = Curve::from_positions(
            1,
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(20.0, 0.0, 0.0),
            ],
            0.8,
            b(),
        );
        c.kind = CurveKind::Line;
        let s = c.sample(2.0);
        assert!(s.samples.len() >= 10);
        for p in s.samples {
            assert!(p.position.y.abs() < 1.0e-5);
            assert!(p.position.z.abs() < 1.0e-5);
        }
    }

    #[test]
    fn circle_approximation_under_one_percent() {
        let c = Curve::circle(2, Vec3::ZERO, 5.0, Vec3::Y, b());
        let s = c.sample(0.5);
        for p in &s.samples {
            let r = Vec3::new(p.position.x, 0.0, p.position.z).length();
            assert!((r - 5.0).abs() / 5.0 < 0.01, "radius {r}");
        }
    }

    #[test]
    fn eval_endpoints_and_midpoint() {
        let c = Curve::from_positions(
            3,
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(20.0, 0.0, 0.0),
                Vec3::new(30.0, 0.0, 0.0),
            ],
            0.7,
            b(),
        )
        .with_kind(CurveKind::Line);
        assert!((c.eval_position(0.0) - Vec3::ZERO).length() < 1e-5);
        assert!((c.eval_position(1.0) - Vec3::new(30.0, 0.0, 0.0)).length() < 1e-4);
        assert!((c.eval_position(0.5) - Vec3::new(15.0, 0.0, 0.0)).length() < 0.05);
    }

    #[test]
    fn arc_length_is_uniform_for_uneven_points() {
        let c = Curve::from_positions(
            4,
            vec![
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(30.0, 0.0, 0.0),
            ],
            0.8,
            b(),
        )
        .with_kind(CurveKind::Line);
        let a = c.eval_position(0.25);
        let bpos = c.eval_position(0.5);
        let cpos = c.eval_position(0.75);
        assert!((a.x - 7.5).abs() < 0.2);
        assert!((bpos.x - 15.0).abs() < 0.2);
        assert!((cpos.x - 22.5).abs() < 0.2);
    }

    #[test]
    fn bezier_midpoint_is_correct() {
        let mut c = Curve::from_positions(5, vec![Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0)], 0.8, b());
        c.kind = CurveKind::Bezier;
        c.points[0].handle_out = Vec3::new(1.0, 2.0, 0.0);
        c.points[1].handle_in = Vec3::new(-1.0, 2.0, 0.0);
        let (mid, _) = c.eval(0.5);
        // The curve is symmetric, so arc-length t=0.5 is also Bezier t=0.5.
        let expected = crate::math::bezier(
            Vec3::ZERO,
            Vec3::new(1.0, 2.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            0.5,
        );
        assert!(
            (mid - expected).length() < 0.01,
            "mid {mid:?} expected {expected:?}"
        );
    }

    #[test]
    fn pressure_interpolates() {
        let mut c = Curve::from_positions(
            6,
            vec![Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 20.0],
            0.2,
            b(),
        );
        c.kind = CurveKind::Line;
        c.points[1].pressure = 1.0;
        let (_, p) = c.eval(0.5);
        assert!(p > 0.3);
    }

    #[test]
    fn sampling_hits_endpoints() {
        let c = Curve::from_positions(
            7,
            vec![
                Vec3::ZERO,
                Vec3::new(5.0, 2.0, 0.0),
                Vec3::new(10.0, 0.0, 0.0),
            ],
            0.8,
            b(),
        );
        let s = c.sample(1.0);
        assert!(s.samples[0].position.distance(Vec3::ZERO) < 1e-5);
        assert!(
            s.samples
                .last()
                .unwrap()
                .position
                .distance(Vec3::new(10.0, 0.0, 0.0))
                < 1e-4
        );
    }

    #[test]
    fn closed_sample_has_no_duplicate_seam() {
        let c = Curve::circle(8, Vec3::ZERO, 4.0, Vec3::Y, b());
        let s = c.sample(0.5);
        assert!(s.closed);
        assert!(
            s.samples[0]
                .position
                .distance(s.samples.last().unwrap().position)
                > 0.1
        );
    }

    #[test]
    fn translate_moves_all_points() {
        let mut c = Curve::line(9, Vec3::ZERO, Vec3::X * 5.0, b());
        c.translate(Vec3::Y * 2.0);
        assert!(c.points.iter().all(|p| (p.position.y - 2.0).abs() < 1e-6));
    }

    #[test]
    fn mirror_reflects_and_reverses() {
        let c = Curve::from_positions(10, vec![Vec3::X * 2.0, Vec3::X * 3.0], 0.8, b());
        let m = mirror_curve(11, &c, Vec3::ZERO, Vec3::X);
        assert!((m.points[0].position.x + 3.0).abs() < 1e-5);
    }

    #[test]
    fn frames_populated() {
        let c = Curve::circle(12, Vec3::ZERO, 2.0, Vec3::Y, b());
        assert!(!c.frames.is_empty());
        assert!(c.frames.iter().all(RmfFrame::is_finite));
    }

    #[test]
    fn tension_changes_curvature() {
        let mut a = Curve::from_positions(
            13,
            vec![
                Vec3::ZERO,
                Vec3::new(10.0, 10.0, 0.0),
                Vec3::new(20.0, 0.0, 0.0),
            ],
            0.8,
            b(),
        );
        let mut c = a.clone();
        a.tension = 0.1;
        c.tension = 0.9;
        a.refresh_frames();
        c.refresh_frames();
        let pa = a.eval_position(0.25);
        let pc = c.eval_position(0.25);
        assert!((pa - pc).length() > 0.01);
    }

    #[test]
    fn normal_is_perpendicular_to_tangent() {
        let c = Curve::from_positions(
            14,
            vec![
                Vec3::ZERO,
                Vec3::new(5.0, 5.0, 5.0),
                Vec3::new(10.0, 0.0, 2.0),
            ],
            0.8,
            b(),
        );
        for s in c.sample(1.0).samples {
            assert!(s.normal.dot(s.tangent).abs() < 1e-4);
        }
    }

    #[test]
    fn ellipse_closure() {
        let c = Curve::ellipse(15, Vec3::ZERO, 6.0, 3.0, 0.3, b());
        let p = c.eval_position(1.0);
        assert!((p.x - c.points[0].position.x).abs() < 1e-3);
        assert!((p.y - c.points[0].position.y).abs() < 1e-3);
    }

    #[test]
    fn adaptive_sampling_respects_max_spacing() {
        let c = Curve::from_positions(
            17,
            vec![
                Vec3::ZERO,
                Vec3::new(40.0, 40.0, 0.0),
                Vec3::new(80.0, 0.0, 0.0),
                Vec3::new(120.0, 40.0, 0.0),
            ],
            0.8,
            b(),
        );
        let samples = c.sample_adaptive(20.0, 2.5);
        assert!(samples.samples.len() > c.sample(20.0).samples.len());
        for pair in samples.samples.windows(2) {
            let gap = pair[0].position.distance(pair[1].position);
            assert!(gap <= 3.0, "adaptive gap {gap} exceeded the cap");
        }
    }

    #[test]
    fn tangent_direction() {
        let c = Curve::line(16, Vec3::ZERO, Vec3::X * 10.0, b());
        let t = c.tangent(0.5);
        assert!((t - Vec3::X).length() < 1e-4);
        let _ = normalize_or(t, Vec3::Y);
        let _ = TAU;
    }
}
