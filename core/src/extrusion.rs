//! Sweep a curve's RMF frames into tube, ribbon or profiled meshes.

use crate::brush::{Brush, BrushInput, BrushProfile, ProfileShape};
use crate::curve::{Curve, CurveSamples};
use crate::mesh::Mesh;
use crate::{Vec2, Vec3};
use std::f32::consts::TAU;

#[derive(Clone, Debug)]
pub struct ExtrusionOptions {
    /// World-space sampling interval along the curve.
    pub spacing: f32,
    /// Force a side count; `None` uses the brush profile count.
    pub sides: Option<u32>,
    pub adaptive_sides: bool,
    pub min_sides: u32,
    pub max_sides: u32,
    pub caps: bool,
    pub weld: bool,
    pub weld_tolerance: f32,
    pub smooth_normals: bool,
    /// Generate a two-vertex-wide ribbon instead of a closed profile.
    pub ribbon: bool,
    pub two_sided: bool,
}

impl Default for ExtrusionOptions {
    fn default() -> Self {
        Self {
            spacing: 2.0,
            sides: None,
            adaptive_sides: true,
            min_sides: 8,
            max_sides: 64,
            caps: true,
            weld: false,
            weld_tolerance: 0.001,
            smooth_normals: true,
            ribbon: false,
            two_sided: false,
        }
    }
}

/// Full mesh build for a curve and its brush.
pub fn extrude_curve(curve: &Curve, options: &ExtrusionOptions) -> Mesh {
    let samples = curve.sample(options.spacing);
    if options.ribbon {
        extrude_ribbon_samples(&samples, &curve.brush, options)
    } else {
        extrude_samples(&samples, &curve.brush, options)
    }
}

/// A cross-section point path. Returned points are closed (no duplicated first
/// point). Coordinates are in units of the brush radius.
pub fn profile_points(profile: &BrushProfile, sides_override: Option<u32>) -> Vec<Vec2> {
    let sides = sides_override.unwrap_or(profile.sides).clamp(3, 64) as usize;
    let mut pts = match profile.shape {
        ProfileShape::Round => {
            let n = sides.max(3);
            (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * TAU;
                    Vec2::new(a.cos(), a.sin())
                })
                .collect::<Vec<_>>()
        }
        ProfileShape::Flat => {
            let a = profile.aspect.clamp(0.01, 4.0);
            vec![
                Vec2::new(1.0, a),
                Vec2::new(-1.0, a),
                Vec2::new(-1.0, -a),
                Vec2::new(1.0, -a),
            ]
        }
        ProfileShape::Tapered => {
            let n = sides.max(3);
            (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * TAU;
                    // A teardrop: wider on one side, sharp on the other.
                    let r = 0.65 + 0.35 * a.cos();
                    Vec2::new(r * a.cos(), r * a.sin())
                })
                .collect()
        }
        ProfileShape::Custom => {
            let mut custom = profile.custom.clone();
            if custom.len() >= 2 && custom.first() == custom.last() {
                custom.pop();
            }
            if custom.len() < 3 {
                return profile_points(&BrushProfile::round(sides as u32), None);
            }
            custom
        }
    };
    if profile.rotation_deg.abs() > 1.0e-6 {
        let (s, c) = profile.rotation_deg.to_radians().sin_cos();
        for p in &mut pts {
            *p = Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
        }
    }
    pts
}

fn adaptive_sides(
    samples: &[crate::curve::CurveSample],
    brush: &Brush,
    options: &ExtrusionOptions,
) -> u32 {
    if let Some(s) = options.sides {
        return s.clamp(3, 256);
    }
    let profile = &brush.profile;
    let base = match profile.shape {
        ProfileShape::Flat => return 4,
        ProfileShape::Tapered => return profile.sides.clamp(3, 8),
        ProfileShape::Custom => return profile.custom.len().max(3) as u32,
        ProfileShape::Round => profile.sides.max(3),
    };
    if !options.adaptive_sides || samples.len() < 3 {
        return base.clamp(options.min_sides.max(3), options.max_sides.max(3));
    }
    let mut max_angle = 0.0f32;
    for w in samples.windows(3) {
        let a = (w[1].tangent - w[0].tangent).length();
        let b = (w[2].tangent - w[1].tangent).length();
        max_angle = max_angle.max(a).max(b);
    }
    let extra = (max_angle * 60.0).round() as u32;
    (base + extra).clamp(
        options.min_sides.max(3),
        options.max_sides.max(options.min_sides),
    )
}

/// Extrude a closed brush profile around the sampled curve.
pub fn extrude_samples(samples: &CurveSamples, brush: &Brush, options: &ExtrusionOptions) -> Mesh {
    let mut mesh = Mesh::new();
    if samples.samples.len() < 2 {
        return mesh;
    }
    let mut path: Vec<crate::curve::CurveSample> = samples.samples.clone();
    let n_sides = adaptive_sides(&path, brush, options) as usize;
    let profile = profile_points(&brush.profile, Some(n_sides as u32));
    let n = profile.len();
    if n < 3 {
        return mesh;
    }
    if samples.closed {
        // Duplicate the seam ring so U can reach 1 without a UV wrap.
        if let Some(first) = path.first().copied() {
            let mut seam = first;
            seam.u = 1.0;
            path.push(seam);
        }
    }
    let mut ring_start = Vec::with_capacity(path.len());
    for sample in &path {
        let dab = brush.resolve(BrushInput {
            pressure: sample.pressure,
            tilt: 0.0,
            velocity: 0.0,
            roll_rad: 0.0,
            u: sample.u,
        });
        let radius = dab.radius;
        let start = mesh.positions.len() as u32;
        ring_start.push(start);
        for j in 0..=n {
            let p = profile[j % n];
            let world =
                sample.position + sample.normal * (p.x * radius) + sample.binormal * (p.y * radius);
            let outward = (sample.normal * p.x + sample.binormal * p.y).normalize_or_zero();
            mesh.add_vertex(world, outward, Vec2::new(sample.u, j as f32 / n as f32));
        }
    }
    // Side quads.
    for r in 0..path.len() - 1 {
        let ring0 = ring_start[r];
        let ring1 = ring_start[r + 1];
        for j in 0..n {
            let a = ring0 + j as u32;
            let b = ring0 + (j + 1) as u32;
            let c = ring1 + j as u32;
            let d = ring1 + (j + 1) as u32;
            mesh.push_triangle(a, c, b);
            mesh.push_triangle(b, c, d);
        }
    }
    // Caps. The sampled first/last frames plus centre vertices make a fan.
    if options.caps && !samples.closed {
        let first = &path[0];
        let last = &path[path.len() - 1];
        let start_centre = mesh.add_vertex(first.position, -first.tangent, Vec2::splat(0.5));
        let end_centre = mesh.add_vertex(last.position, last.tangent, Vec2::splat(0.5));
        let s0 = ring_start[0];
        let s1 = ring_start[path.len() - 1];
        for j in 0..n {
            let a = s0 + j as u32;
            let b = s0 + (j + 1) as u32;
            mesh.push_triangle(start_centre, b, a);
            let c = s1 + j as u32;
            let d = s1 + (j + 1) as u32;
            mesh.push_triangle(end_centre, c, d);
        }
    }
    if options.weld {
        mesh.weld(options.weld_tolerance);
    }
    if options.smooth_normals {
        mesh.compute_smooth_normals();
    }
    fix_degenerate_normals(&mut mesh);
    mesh
}

/// Extrude a two-vertex ribbon. Width is the brush diameter; thickness is zero.
pub fn extrude_ribbon_samples(
    samples: &CurveSamples,
    brush: &Brush,
    options: &ExtrusionOptions,
) -> Mesh {
    let mut mesh = Mesh::new();
    if samples.samples.len() < 2 {
        return mesh;
    }
    let mut path = samples.samples.clone();
    if samples.closed {
        if let Some(first) = path.first().copied() {
            let mut seam = first;
            seam.u = 1.0;
            path.push(seam);
        }
    }
    let mut left = Vec::with_capacity(path.len());
    let mut right = Vec::with_capacity(path.len());
    for sample in &path {
        let dab = brush.resolve(BrushInput {
            pressure: sample.pressure,
            u: sample.u,
            ..BrushInput::default()
        });
        let width = dab.radius * 2.0;
        // Flat brushes use aspect as a thickness ratio; the visible width stays
        // the full brush diameter.
        let direction = if brush.profile.rotation_deg.abs() > 1.0e-6 {
            let q = crate::Quat::from_axis_angle(
                sample.tangent,
                brush.profile.rotation_deg.to_radians(),
            );
            q * sample.normal
        } else {
            sample.normal
        };
        let half = direction * (width * 0.5);
        left.push(sample.position - half);
        right.push(sample.position + half);
    }
    let base = 0u32;
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        let u = path[i].u;
        mesh.add_vertex(*l, Vec3::ZERO, Vec2::new(u, 0.0));
        mesh.add_vertex(*r, Vec3::ZERO, Vec2::new(u, 1.0));
    }
    for i in 0..path.len() - 1 {
        let l0 = base + (i * 2) as u32;
        let r0 = l0 + 1;
        let l1 = l0 + 2;
        let r1 = l0 + 3;
        mesh.push_triangle(l0, l1, r0);
        mesh.push_triangle(r0, l1, r1);
        if options.two_sided {
            // Back faces so the ribbon reads from both sides when requested.
            mesh.push_triangle(r0, l1, l0);
            mesh.push_triangle(r1, l1, r0);
        }
    }
    mesh.compute_smooth_normals();
    // A ribbon can have face normal flips at high curvature; orient every
    // normal to the ribbon's local surface as best we can.
    for i in 0..path.len() {
        let l = left[i];
        let r = right[i];
        let t = path[i].tangent;
        let n = (r - l).cross(t).normalize_or_zero();
        mesh.normals[i * 2] = n;
        mesh.normals[i * 2 + 1] = n;
    }
    if options.weld {
        mesh.weld(options.weld_tolerance);
    }
    fix_degenerate_normals(&mut mesh);
    mesh
}

fn fix_degenerate_normals(mesh: &mut Mesh) {
    if mesh.normals.len() != mesh.positions.len() {
        mesh.normals = vec![Vec3::Y; mesh.positions.len()];
    }
    let tri_count = mesh.triangle_count();
    let fallback: Vec<Vec3> = (0..mesh.normals.len())
        .map(|i| {
            if tri_count > 0 {
                mesh.face_normal((i / 3).min(tri_count - 1))
            } else {
                Vec3::Y
            }
        })
        .collect();
    for (i, n) in mesh.normals.iter_mut().enumerate() {
        if !n.is_finite() || n.length_squared() < 1.0e-12 {
            *n = fallback[i];
        }
    }
}

/// Quick cross-section polygon used for brush previews in the UI.
pub fn profile_preview(profile: &BrushProfile, sides: u32) -> Vec<Vec2> {
    profile_points(profile, Some(sides.max(3)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Taper;
    use crate::curve::Curve;

    fn brush(size: f32) -> Brush {
        let mut b = Brush {
            base_size: size,
            min_size: 0.001,
            max_size: size * 2.0,
            taper: Taper {
                enabled: false,
                ..Taper::default()
            },
            ..Brush::default()
        };
        b.dynamics.pressure_size = 0.0;
        b.stabilization.strength = 0.0;
        b
    }

    #[test]
    fn straight_tube_has_constant_radius() {
        let mut b = brush(2.0);
        b.profile = BrushProfile::round(16);
        let c = Curve::line(1, Vec3::ZERO, Vec3::X * 40.0, b.clone());
        let opts = ExtrusionOptions {
            spacing: 2.0,
            sides: Some(16),
            adaptive_sides: false,
            caps: false,
            smooth_normals: false,
            ..ExtrusionOptions::default()
        };
        let mesh = extrude_curve(&c, &opts);
        assert!(mesh.positions.len() > 100);
        for p in &mesh.positions {
            let r = Vec3::new(0.0, p.y, p.z).length();
            assert!((r - 1.0).abs() < 1e-3, "radius {r}");
        }
        let samples = c.sample(2.0).samples.len();
        assert_eq!(mesh.positions.len(), samples * 17);
        mesh.validate().unwrap();
    }

    #[test]
    fn pressure_taper_follows_curve() {
        let mut b = brush(6.0);
        b.profile = BrushProfile::round(12);
        b.dynamics.pressure_size = 1.0;
        b.taper.enabled = false;
        b.dynamics.pressure_curve = vec![Vec2::new(0.0, 0.1), Vec2::new(1.0, 1.0)];
        let mut c = Curve::from_positions(
            2,
            vec![
                Vec3::ZERO,
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(20.0, 0.0, 0.0),
            ],
            0.1,
            b,
        );
        c.kind = crate::curve::CurveKind::Line;
        c.points[0].pressure = 0.1;
        c.points[1].pressure = 0.5;
        c.points[2].pressure = 1.0;
        let opts = ExtrusionOptions {
            sides: Some(12),
            adaptive_sides: false,
            caps: false,
            spacing: 2.0,
            ..ExtrusionOptions::default()
        };
        let samples = c.sample(opts.spacing);
        let mesh = extrude_samples(&samples, &c.brush, &opts);
        let ring = 13usize;
        let first_radius = Vec3::new(0.0, mesh.positions[0].y, mesh.positions[0].z).length();
        let last_idx = (samples.samples.len() - 1) * ring;
        let last_radius =
            Vec3::new(0.0, mesh.positions[last_idx].y, mesh.positions[last_idx].z).length();
        assert!(
            last_radius > first_radius * 2.0,
            "{first_radius} {last_radius}"
        );
    }

    #[test]
    fn rectangular_profile_has_four_sides() {
        let mut b = brush(4.0);
        b.profile = BrushProfile::flat(0.3);
        let c = Curve::line(3, Vec3::ZERO, Vec3::X * 10.0, b);
        let opts = ExtrusionOptions {
            sides: None,
            caps: false,
            smooth_normals: false,
            ..ExtrusionOptions::default()
        };
        let mesh = extrude_curve(&c, &opts);
        let samples = c.sample(opts.spacing).samples.len();
        assert_eq!(mesh.positions.len(), samples * 5);
    }

    #[test]
    fn triangular_profile_has_three_sides() {
        let mut b = brush(4.0);
        b.profile = BrushProfile::tapered();
        let c = Curve::line(4, Vec3::ZERO, Vec3::X * 10.0, b);
        let opts = ExtrusionOptions {
            sides: None,
            caps: false,
            smooth_normals: false,
            ..ExtrusionOptions::default()
        };
        let mesh = extrude_curve(&c, &opts);
        let samples = c.sample(opts.spacing).samples.len();
        assert_eq!(mesh.positions.len(), samples * 4);
    }

    #[test]
    fn custom_profile_uses_points() {
        let profile = BrushProfile::custom(vec![
            Vec2::new(-1.0, 0.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
        ]);
        let pts = profile_points(&profile, None);
        assert_eq!(pts.len(), 3);
    }

    #[test]
    fn ribbon_is_flat_and_double_sided() {
        let mut b = brush(4.0);
        b.profile = BrushProfile::flat(0.05);
        let c = Curve::line(5, Vec3::ZERO, Vec3::X * 20.0, b);
        let opts = ExtrusionOptions {
            ribbon: true,
            two_sided: true,
            ..ExtrusionOptions::default()
        };
        let mesh = extrude_curve(&c, &opts);
        assert!(mesh.positions.iter().all(|p| p.z.abs() < 1e-4));
        assert_eq!(mesh.triangle_count() % 2, 0);
        assert!(mesh.triangle_count() > 10);
    }

    #[test]
    fn ribbon_two_sided_flag_controls_back_faces() {
        let mut b = brush(4.0);
        b.profile = BrushProfile::flat(0.05);
        let c = Curve::line(7, Vec3::ZERO, Vec3::X * 20.0, b);
        let single = extrude_curve(
            &c,
            &ExtrusionOptions {
                ribbon: true,
                two_sided: false,
                ..ExtrusionOptions::default()
            },
        );
        let double = extrude_curve(
            &c,
            &ExtrusionOptions {
                ribbon: true,
                two_sided: true,
                ..ExtrusionOptions::default()
            },
        );
        assert_eq!(double.triangle_count(), single.triangle_count() * 2);
    }

    #[test]
    fn closed_circle_has_no_end_caps() {
        let b = brush(2.0);
        let c = Curve::circle(6, Vec3::ZERO, 10.0, Vec3::Y, b);
        let opts = ExtrusionOptions {
            caps: true,
            ..ExtrusionOptions::default()
        };
        let mesh = extrude_curve(&c, &opts);
        assert!(mesh.positions.iter().all(|p| p.is_finite()));
        assert!(mesh.normals.iter().all(|n| (n.length() - 1.0).abs() < 0.05));
        mesh.validate().unwrap();
    }

    #[test]
    fn thousand_point_curve_optimizes() {
        let mut b = brush(3.0);
        b.profile = BrushProfile::round(10);
        let mut points = Vec::new();
        for i in 0..1000 {
            let t = i as f32 / 999.0;
            points.push(Vec3::new(
                t * 500.0,
                (t * 12.0).sin() * 20.0,
                (t * 8.0).cos() * 20.0,
            ));
        }
        let c = Curve::from_positions(7, points, 0.8, b);
        let opts = ExtrusionOptions {
            spacing: 1.0,
            sides: Some(10),
            adaptive_sides: false,
            caps: true,
            ..ExtrusionOptions::default()
        };
        let start = std::time::Instant::now();
        let mut mesh = extrude_curve(&c, &opts);
        let elapsed = start.elapsed().as_secs_f32();
        assert!(elapsed < 2.0, "mesh build took {elapsed}s");
        assert!(mesh.vertex_count() > 1000);
        assert!(mesh.normals.iter().all(|n| (n.length() - 1.0).abs() < 0.1));
        assert!(mesh.positions.iter().all(|p| p.is_finite()));
        mesh.optimize(0.001);
        mesh.validate().unwrap();
    }

    #[test]
    fn all_brush_profiles_build_meshes() {
        for brush in crate::brush::builtin_brushes() {
            let c = Curve::line(8, Vec3::ZERO, Vec3::X * 25.0, brush);
            let mesh = extrude_curve(&c, &ExtrusionOptions::default());
            assert!(!mesh.is_empty());
            assert!(mesh.normals.iter().all(|n| n.is_finite()));
        }
    }

    #[test]
    fn adaptive_sides_increase_on_curve() {
        let mut b = brush(2.0);
        b.profile = BrushProfile::round(8);
        let c = Curve::circle(9, Vec3::ZERO, 5.0, Vec3::Y, b);
        let samples = c.sample(2.0);
        let opts = ExtrusionOptions {
            adaptive_sides: true,
            min_sides: 8,
            max_sides: 64,
            ..ExtrusionOptions::default()
        };
        let sides = adaptive_sides(&samples.samples, &c.brush, &opts);
        assert!(sides >= 8);
    }
}
