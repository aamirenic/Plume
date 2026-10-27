//! Transform tools: translate/rotate/scale, alignment, distribution and arrays.
//!
//! The viewport uses screen-space gizmo handles; this module owns the math so
//! the desktop app and a future touch UI behave identically.

use crate::scene::{MirrorAxis, Scene};
use crate::viewport::Camera;
use crate::{Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn label(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }

    pub fn vector(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }

    pub fn mirror_axis(self) -> MirrorAxis {
        match self {
            Axis::X => MirrorAxis::X,
            Axis::Y => MirrorAxis::Y,
            Axis::Z => MirrorAxis::Z,
        }
    }

    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransformMode {
    Translate,
    Rotate,
    Scale,
}

impl TransformMode {
    pub fn label(self) -> &'static str {
        match self {
            TransformMode::Translate => "Move",
            TransformMode::Rotate => "Rotate",
            TransformMode::Scale => "Scale",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            TransformMode::Translate => "✥",
            TransformMode::Rotate => "⟳",
            TransformMode::Scale => "⤢",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapMode {
    Off,
    Grid,
    Angle15,
    Angle45,
}

impl SnapMode {
    pub fn label(self) -> &'static str {
        match self {
            SnapMode::Off => "Off",
            SnapMode::Grid => "Grid",
            SnapMode::Angle15 => "15°",
            SnapMode::Angle45 => "45°",
        }
    }

    pub fn translation(self, delta: Vec3, grid: f32) -> Vec3 {
        match self {
            SnapMode::Grid => {
                let g = grid.max(1.0e-4);
                Vec3::new(
                    (delta.x / g).round() * g,
                    (delta.y / g).round() * g,
                    (delta.z / g).round() * g,
                )
            }
            _ => delta,
        }
    }

    pub fn rotation(self, radians: f32) -> f32 {
        match self {
            SnapMode::Angle15 => {
                (radians / (15.0f32).to_radians()).round() * (15.0f32).to_radians()
            }
            SnapMode::Angle45 => {
                (radians / (45.0f32).to_radians()).round() * (45.0f32).to_radians()
            }
            _ => radians,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoHandle {
    pub axis: Axis,
    pub start: Vec2,
    pub end: Vec2,
}

/// Compute screen-space gizmo handles around `centre`. Pixel length shrinks
/// gently with distance so the gizmo stays usable in large scenes.
pub fn gizmo_handles(
    centre: Vec3,
    camera: &Camera,
    viewport: Vec2,
    mode: TransformMode,
) -> Vec<GizmoHandle> {
    let Some(start) = camera.project(centre, viewport) else {
        return Vec::new();
    };
    let length = if mode == TransformMode::Translate {
        92.0
    } else {
        78.0
    };
    let dist = camera.distance().max(1.0);
    let scale = (280.0 / dist).clamp(0.45, 1.25);
    let mut out = Vec::new();
    for axis in Axis::ALL {
        let dir = axis.vector();
        let endpoint = centre + dir * ((length * scale) / viewport.y.max(1.0) * dist * 0.8);
        if let Some(end) = camera.project(endpoint, viewport) {
            if end.distance(start) > 18.0 {
                out.push(GizmoHandle { axis, start, end });
                continue;
            }
        }
        // Axis points nearly at the camera: fall back to a stable screen
        // direction so the handle remains available.
        let screen_dir = match axis {
            Axis::X => Vec2::X,
            Axis::Y => -Vec2::Y,
            Axis::Z => Vec2::new(0.55, 0.55),
        };
        out.push(GizmoHandle {
            axis,
            start,
            end: start + screen_dir * length * scale,
        });
    }
    out
}

pub fn hit_gizmo(handles: &[GizmoHandle], screen: Vec2, tolerance: f32) -> Option<GizmoHandle> {
    handles
        .iter()
        .map(|h| {
            let d = distance_to_segment(screen, h.start, h.end);
            (*h, d)
        })
        .filter(|(_, d)| *d <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(h, _)| h)
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 < 1.0e-8 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Project a screen drag onto a world axis through the gizmo centre.
pub fn axis_drag_amount(
    camera: &Camera,
    viewport: Vec2,
    centre: Vec3,
    axis: Vec3,
    start_screen: Vec2,
    screen: Vec2,
) -> f32 {
    let (right, up, _) = camera.basis();
    let delta = screen - start_screen;
    // Small drags are mapped using the camera-plane projection of the axis;
    // this is stable for axes pointing at the camera as well.
    let axis_screen = Vec2::new(axis.dot(right), -axis.dot(up));
    let denom = axis_screen.length_squared();
    if denom < 1.0e-4 {
        let world_per_px = 2.0 * camera.distance() * (camera.fov_y_deg.to_radians() * 0.5).tan()
            / viewport.y.max(1.0);
        return delta.y * world_per_px;
    }
    let depth_scale =
        2.0 * camera.distance() * (camera.fov_y_deg.to_radians() * 0.5).tan() / viewport.y.max(1.0);
    let _ = centre;
    delta.dot(axis_screen) / denom * depth_scale
}

pub fn translate_matrix(axis: Axis, amount: f32) -> Mat4 {
    Mat4::from_translation(axis.vector() * amount)
}

pub fn rotate_matrix(axis: Axis, radians: f32, pivot: Vec3) -> Mat4 {
    Mat4::from_translation(pivot)
        * Mat4::from_quat(Quat::from_axis_angle(axis.vector(), radians))
        * Mat4::from_translation(-pivot)
}

pub fn scale_matrix(axis: Axis, factor: f32, pivot: Vec3) -> Mat4 {
    let f = factor.max(0.01);
    let mut s = Vec3::ONE;
    match axis {
        Axis::X => s.x = f,
        Axis::Y => s.y = f,
        Axis::Z => s.z = f,
    }
    Mat4::from_translation(pivot) * Mat4::from_scale(s) * Mat4::from_translation(-pivot)
}

pub fn uniform_scale_matrix(factor: f32, pivot: Vec3) -> Mat4 {
    let f = factor.max(0.01);
    Mat4::from_translation(pivot)
        * Mat4::from_scale(Vec3::splat(f))
        * Mat4::from_translation(-pivot)
}

/// Apply a world-space translation to selected curves.
pub fn translate_selection(scene: &mut Scene, delta: Vec3, snap: SnapMode, grid: f32) -> usize {
    let delta = snap.translation(delta, grid);
    scene.translate_selection(delta)
}

/// Alignment helpers for multi-curve selections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlignMode {
    Left,
    CentreX,
    Right,
    Bottom,
    CentreY,
    Top,
    Front,
    CentreZ,
    Back,
}

impl AlignMode {
    pub fn label(self) -> &'static str {
        match self {
            AlignMode::Left => "Left",
            AlignMode::CentreX => "Centre X",
            AlignMode::Right => "Right",
            AlignMode::Bottom => "Bottom",
            AlignMode::CentreY => "Centre Y",
            AlignMode::Top => "Top",
            AlignMode::Front => "Front",
            AlignMode::CentreZ => "Centre Z",
            AlignMode::Back => "Back",
        }
    }

    pub const ALL: [AlignMode; 9] = [
        AlignMode::Left,
        AlignMode::CentreX,
        AlignMode::Right,
        AlignMode::Bottom,
        AlignMode::CentreY,
        AlignMode::Top,
        AlignMode::Front,
        AlignMode::CentreZ,
        AlignMode::Back,
    ];
}

/// Align selected curves to their combined bounds and return the count moved.
pub fn align_selection(scene: &mut Scene, mode: AlignMode) -> usize {
    let Some((min, max)) = scene.selection_bounds() else {
        return 0;
    };
    let centre = (min + max) * 0.5;
    let ids: Vec<_> = scene.selection.curves.iter().copied().collect();
    let mut count = 0;
    for id in ids {
        if let Some(curve) = scene.curves.iter_mut().find(|c| c.id == id) {
            if curve.locked || curve.points.is_empty() {
                continue;
            }
            let (a, b) = curve.bounds();
            let c = (a + b) * 0.5;
            let delta = match mode {
                AlignMode::Left => Vec3::new(min.x - a.x, 0.0, 0.0),
                AlignMode::CentreX => Vec3::new(centre.x - c.x, 0.0, 0.0),
                AlignMode::Right => Vec3::new(max.x - b.x, 0.0, 0.0),
                AlignMode::Bottom => Vec3::new(0.0, min.y - a.y, 0.0),
                AlignMode::CentreY => Vec3::new(0.0, centre.y - c.y, 0.0),
                AlignMode::Top => Vec3::new(0.0, max.y - b.y, 0.0),
                AlignMode::Front => Vec3::new(0.0, 0.0, min.z - a.z),
                AlignMode::CentreZ => Vec3::new(0.0, 0.0, centre.z - c.z),
                AlignMode::Back => Vec3::new(0.0, 0.0, max.z - b.z),
            };
            curve.translate(delta);
            count += 1;
        }
    }
    if count > 0 {
        scene.touch();
    }
    count
}

/// Distribute selected curves evenly along the chosen axis.
pub fn distribute_selection(scene: &mut Scene, axis: Axis) -> usize {
    let mut ids: Vec<_> = scene
        .selection
        .curves
        .iter()
        .copied()
        .filter(|id| {
            scene
                .curves
                .iter()
                .find(|c| c.id == *id)
                .map(|c| !c.locked)
                .unwrap_or(false)
        })
        .collect();
    if ids.len() < 3 {
        return 0;
    }
    let key = |c: &crate::curve::Curve| c.centre().dot(axis.vector());
    ids.sort_by(|a, b| {
        let ca = scene.curves.iter().find(|c| c.id == *a).unwrap();
        let cb = scene.curves.iter().find(|c| c.id == *b).unwrap();
        key(ca).total_cmp(&key(cb))
    });
    let first = scene
        .curves
        .iter()
        .find(|c| c.id == ids[0])
        .unwrap()
        .centre();
    let last = scene
        .curves
        .iter()
        .find(|c| c.id == *ids.last().unwrap())
        .unwrap()
        .centre();
    let start = first.dot(axis.vector());
    let end = last.dot(axis.vector());
    let n = ids.len();
    let mut count = 0;
    for (i, id) in ids.iter().enumerate().skip(1).take(n - 2) {
        let target = start + (end - start) * i as f32 / (n - 1) as f32;
        let curve = scene.curves.iter_mut().find(|c| c.id == *id).unwrap();
        let current = curve.centre().dot(axis.vector());
        curve.translate(axis.vector() * (target - current));
        count += 1;
    }
    if count > 0 {
        scene.touch();
    }
    count
}

/// Duplicate the selection in a grid. Returns the number of new curves.
pub fn array_selection(
    scene: &mut Scene,
    count: u32,
    offset: Vec3,
    rotate_deg: f32,
    axis: Axis,
) -> usize {
    let ids: Vec<_> = scene.selection.curves.iter().copied().collect();
    if ids.is_empty() || count < 1 {
        return 0;
    }
    let count = count.min(256);
    let mut created = 0;
    for step in 1..=count {
        let q = Quat::from_axis_angle(axis.vector(), rotate_deg.to_radians() * step as f32);
        let centre = scene
            .selection_bounds()
            .map(|(a, b)| (a + b) * 0.5)
            .unwrap_or(Vec3::ZERO);
        for id in &ids {
            let Some(curve) = scene.curves.iter().find(|c| c.id == *id).cloned() else {
                continue;
            };
            let new_id = scene.new_curve_id();
            let mut copy = curve;
            copy.id = new_id;
            copy.name = format!("{} {step}", copy.name);
            let rot = Mat4::from_translation(centre)
                * Mat4::from_quat(q)
                * Mat4::from_translation(-centre);
            let mat = rot * Mat4::from_translation(offset * step as f32);
            copy.transform(mat);
            copy.tag = Some("array".into());
            scene.curves.push(copy);
            created += 1;
        }
    }
    scene.touch();
    created
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;

    fn scene_with_two() -> (Scene, crate::CurveId, crate::CurveId) {
        let mut scene = Scene::new();
        let a = scene.new_curve_id();
        let b = scene.new_curve_id();
        scene.add_curve(Curve::line(
            a,
            Vec3::new(-20.0, 0.0, 0.0),
            Vec3::new(-10.0, 0.0, 0.0),
            Brush::default(),
        ));
        scene.add_curve(Curve::line(
            b,
            Vec3::new(10.0, 4.0, 0.0),
            Vec3::new(20.0, 4.0, 0.0),
            Brush::default(),
        ));
        scene.selection.curves.insert(a);
        scene.selection.curves.insert(b);
        (scene, a, b)
    }

    #[test]
    fn snap_translation_rounds_to_grid() {
        let d = SnapMode::Grid.translation(Vec3::new(7.4, -2.2, 0.1), 5.0);
        assert!((d.x - 5.0).abs() < 1e-5);
        assert!((d.y + 0.0).abs() < 1e-5);
        assert!((d.z - 0.0).abs() < 1e-5);
    }

    #[test]
    fn snap_rotation_uses_degrees() {
        let r = SnapMode::Angle45.rotation(50.0f32.to_radians());
        assert!((r.to_degrees() - 45.0).abs() < 1e-4);
    }

    #[test]
    fn gizmo_handles_are_available() {
        let cam = Camera::new(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO, 45.0);
        let handles = gizmo_handles(
            Vec3::ZERO,
            &cam,
            Vec2::new(800.0, 600.0),
            TransformMode::Translate,
        );
        assert_eq!(handles.len(), 3);
        assert!(handles
            .iter()
            .all(|h| h.start.is_finite() && h.end.is_finite()));
    }

    #[test]
    fn alignment_moves_curves() {
        let (mut scene, a, _b) = scene_with_two();
        align_selection(&mut scene, AlignMode::Left);
        let bounds = scene.curve(a).unwrap().bounds();
        let all = scene.selection_bounds().unwrap();
        assert!((bounds.0.x - all.0.x).abs() < 1e-4);
    }

    #[test]
    fn distribute_keeps_endpoints() {
        let mut scene = Scene::new();
        for (i, x) in [0.0, 5.0, 100.0].iter().enumerate() {
            let id = scene.new_curve_id();
            let c = Curve::line(
                id,
                Vec3::new(*x, 0.0, 0.0),
                Vec3::new(*x + 2.0, 0.0, 0.0),
                Brush::default(),
            );
            scene.add_curve(c);
            scene.selection.curves.insert(id);
            let _ = i;
        }
        assert_eq!(distribute_selection(&mut scene, Axis::X), 1);
        let centres: Vec<f32> = scene
            .selection
            .curves
            .iter()
            .map(|id| scene.curve(*id).unwrap().centre().x)
            .collect();
        assert!(centres.iter().any(|x| (*x - 51.0).abs() < 0.1));
    }

    #[test]
    fn array_creates_requested_copies() {
        let (mut scene, _, _) = scene_with_two();
        let before = scene.curves.len();
        let created = array_selection(&mut scene, 3, Vec3::Y * 10.0, 0.0, Axis::Y);
        assert_eq!(created, 6);
        assert_eq!(scene.curves.len(), before + 6);
    }

    #[test]
    fn rotate_matrix_keeps_pivot() {
        let p = Vec3::new(5.0, 2.0, -3.0);
        let m = rotate_matrix(Axis::Y, 1.2, p);
        assert!((m.transform_point3(p) - p).length() < 1e-5);
    }
}
