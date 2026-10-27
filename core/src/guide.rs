//! Guide surfaces: planes, primitives, freeform and lofted sheets.
//!
//! Every guide owns a local-space triangle mesh and a cached BVH. Transforms
//! are applied to the ray (instead of rebuilding the BVH), so dragging a guide
//! stays cheap.

use crate::bvh::{Bvh, RayHit};
use crate::mesh::Mesh;
use crate::viewport::Ray;
use crate::{GuideId, Mat4, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GuideKind {
    Plane {
        width: f32,
        height: f32,
    },
    Box {
        size: Vec3,
    },
    Sphere {
        radius: f32,
        segments: u32,
        rings: u32,
    },
    Cylinder {
        radius: f32,
        height: f32,
        segments: u32,
    },
    Cone {
        radius: f32,
        height: f32,
        segments: u32,
    },
    Torus {
        major_radius: f32,
        minor_radius: f32,
        major_segments: u32,
        minor_segments: u32,
    },
    Freeform {
        rows: Vec<Vec<Vec3>>,
        subdivisions: u32,
    },
    Loft {
        rows: Vec<Vec<Vec3>>,
        source_curves: Vec<crate::CurveId>,
    },
    Imported {
        source_path: Option<String>,
        mesh: Mesh,
    },
}

impl GuideKind {
    pub fn label(&self) -> &'static str {
        match self {
            GuideKind::Plane { .. } => "Plane",
            GuideKind::Box { .. } => "Box",
            GuideKind::Sphere { .. } => "Sphere",
            GuideKind::Cylinder { .. } => "Cylinder",
            GuideKind::Cone { .. } => "Cone",
            GuideKind::Torus { .. } => "Torus",
            GuideKind::Freeform { .. } => "Freeform",
            GuideKind::Loft { .. } => "Loft",
            GuideKind::Imported { .. } => "Imported",
        }
    }

    pub fn icon_key(&self) -> &'static str {
        match self {
            GuideKind::Plane { .. } => "plane",
            GuideKind::Box { .. } => "box",
            GuideKind::Sphere { .. } => "sphere",
            GuideKind::Cylinder { .. } => "cylinder",
            GuideKind::Cone { .. } => "cone",
            GuideKind::Torus { .. } => "torus",
            GuideKind::Freeform { .. } => "freeform",
            GuideKind::Loft { .. } => "loft",
            GuideKind::Imported { .. } => "import",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Guide {
    pub id: GuideId,
    pub name: String,
    pub kind: GuideKind,
    pub transform: Mat4,
    pub visible: bool,
    /// 0..1
    pub opacity: f32,
    pub color: Vec4,
    pub active: bool,
    /// Render the triangle surface translucent instead of only wireframe.
    pub solid: bool,
    pub locked: bool,
    #[serde(skip)]
    pub mesh_cache: Option<Mesh>,
    #[serde(skip)]
    pub bvh_cache: Option<Bvh>,
}

impl Guide {
    pub fn new(id: GuideId, name: impl Into<String>, kind: GuideKind) -> Self {
        let mut g = Self {
            id,
            name: name.into(),
            kind,
            transform: Mat4::IDENTITY,
            visible: true,
            opacity: 0.45,
            color: Vec4::new(0.36, 0.78, 0.86, 1.0),
            active: false,
            solid: false,
            locked: false,
            mesh_cache: None,
            bvh_cache: None,
        };
        g.rebuild();
        g
    }

    pub fn sphere(id: GuideId, centre: Vec3, radius: f32) -> Self {
        let mut g = Self::new(
            id,
            "Sphere guide",
            GuideKind::Sphere {
                radius,
                segments: 40,
                rings: 24,
            },
        );
        g.transform = Mat4::from_translation(centre);
        g.rebuild();
        g
    }

    pub fn plane(id: GuideId, centre: Vec3, normal: Vec3) -> Self {
        let mut g = Self::new(
            id,
            "Plane guide",
            GuideKind::Plane {
                width: 300.0,
                height: 300.0,
            },
        );
        let rotation = Mat4::from_quat(crate::math::rotation_between(Vec3::Y, normal));
        g.transform = Mat4::from_translation(centre) * rotation;
        g.rebuild();
        g
    }

    pub fn cylinder(id: GuideId, centre: Vec3, radius: f32, height: f32) -> Self {
        let mut g = Self::new(
            id,
            "Cylinder guide",
            GuideKind::Cylinder {
                radius,
                height,
                segments: 40,
            },
        );
        g.transform = Mat4::from_translation(centre);
        g.rebuild();
        g
    }

    pub fn cone(id: GuideId, centre: Vec3, radius: f32, height: f32) -> Self {
        let mut g = Self::new(
            id,
            "Cone guide",
            GuideKind::Cone {
                radius,
                height,
                segments: 40,
            },
        );
        g.transform = Mat4::from_translation(centre);
        g.rebuild();
        g
    }

    pub fn torus(id: GuideId, centre: Vec3, major: f32, minor: f32) -> Self {
        let mut g = Self::new(
            id,
            "Torus guide",
            GuideKind::Torus {
                major_radius: major,
                minor_radius: minor,
                major_segments: 40,
                minor_segments: 20,
            },
        );
        g.transform = Mat4::from_translation(centre);
        g.rebuild();
        g
    }

    pub fn box_guide(id: GuideId, centre: Vec3, size: Vec3) -> Self {
        let mut g = Self::new(id, "Box guide", GuideKind::Box { size });
        g.transform = Mat4::from_translation(centre);
        g.rebuild();
        g
    }

    pub fn freeform(id: GuideId, rows: Vec<Vec<Vec3>>, subdivisions: u32) -> Self {
        let mut g = Self::new(
            id,
            "Freeform guide",
            GuideKind::Freeform { rows, subdivisions },
        );
        g.rebuild();
        g
    }

    pub fn loft(id: GuideId, rows: Vec<Vec<Vec3>>, source_curves: Vec<crate::CurveId>) -> Self {
        let mut g = Self::new(
            id,
            "Loft guide",
            GuideKind::Loft {
                rows,
                source_curves,
            },
        );
        g.color = Vec4::new(0.82, 0.62, 0.98, 1.0);
        g.rebuild();
        g
    }

    pub fn imported(
        id: GuideId,
        name: impl Into<String>,
        mesh: Mesh,
        path: Option<String>,
    ) -> Self {
        let mut g = Self::new(
            id,
            name,
            GuideKind::Imported {
                source_path: path,
                mesh,
            },
        );
        g.rebuild();
        g
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Rebuild the local mesh and its BVH. Called on creation, edit and load.
    pub fn rebuild(&mut self) {
        let mesh = build_guide_mesh(&self.kind);
        self.bvh_cache = Some(Bvh::build(&mesh));
        self.mesh_cache = Some(mesh);
    }

    pub fn mesh(&mut self) -> &Mesh {
        if self.mesh_cache.is_none() {
            self.rebuild();
        }
        self.mesh_cache.as_ref().unwrap()
    }

    pub fn bvh(&mut self) -> &Bvh {
        if self.bvh_cache.is_none() {
            self.rebuild();
        }
        self.bvh_cache.as_ref().unwrap()
    }

    pub fn world_matrix(&self) -> Mat4 {
        self.transform
    }

    pub fn transform(&mut self, mat: Mat4) {
        self.transform = mat * self.transform;
    }

    pub fn translate(&mut self, delta: Vec3) {
        self.transform = Mat4::from_translation(delta) * self.transform;
    }

    /// Ray cast in world space. The ray is moved into guide-local space, which
    /// avoids rebuilding the BVH when the guide is transformed.
    pub fn raycast(&mut self, ray: &Ray) -> Option<GuideHit> {
        if !self.visible {
            return None;
        }
        let inv = self.transform.inverse();
        if !inv.is_finite() {
            return None;
        }
        let local_origin = inv.transform_point3(ray.origin);
        let local_dir = inv.transform_vector3(ray.dir);
        if local_dir.length_squared() < 1.0e-12 {
            return None;
        }
        let local_ray = Ray {
            origin: local_origin,
            dir: local_dir.normalize(),
        };
        let hit = self.bvh().raycast(&local_ray)?;
        let world_point = self.transform.transform_point3(hit.point);
        let normal_mat = crate::Mat3::from_mat4(self.transform).inverse().transpose();
        let world_normal = (normal_mat * hit.normal).normalize_or_zero();
        let distance = (world_point - ray.origin).dot(ray.dir).max(0.0);
        Some(GuideHit {
            guide_id: self.id,
            distance,
            point: world_point,
            normal: world_normal,
            uv: hit.uv,
            triangle: hit.triangle,
            local_hit: hit,
        })
    }

    /// World-space bounds of the guide mesh.
    pub fn world_bounds(&mut self) -> (Vec3, Vec3) {
        let transform = self.transform;
        let mesh = self.mesh();
        if mesh.positions.is_empty() {
            return (Vec3::ZERO, Vec3::ZERO);
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in &mesh.positions {
            let w = transform.transform_point3(*p);
            min = min.min(w);
            max = max.max(w);
        }
        (min, max)
    }

    pub fn world_mesh(&mut self) -> Mesh {
        let mut mesh = self.mesh().clone();
        mesh.transform(self.transform);
        mesh
    }

    pub fn kind_bounds(&self) -> (Vec3, Vec3) {
        let m = build_guide_mesh(&self.kind);
        m.bounds()
    }
}

#[derive(Clone, Debug)]
pub struct GuideHit {
    pub guide_id: GuideId,
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    pub triangle: u32,
    pub local_hit: RayHit,
}

#[derive(Clone, Debug)]
pub struct ActiveGuideHit {
    pub guide_index: usize,
    pub hit: GuideHit,
}

/// Cast a ray against the active guide only.
pub fn raycast_active(guides: &mut [Guide], ray: &Ray) -> Option<ActiveGuideHit> {
    let mut best: Option<ActiveGuideHit> = None;
    for (index, guide) in guides.iter_mut().enumerate() {
        if !guide.active || !guide.visible {
            continue;
        }
        if let Some(hit) = guide.raycast(ray) {
            if best
                .as_ref()
                .map(|b| hit.distance < b.hit.distance)
                .unwrap_or(true)
            {
                best = Some(ActiveGuideHit {
                    guide_index: index,
                    hit,
                });
            }
        }
    }
    best
}

pub fn build_guide_mesh(kind: &GuideKind) -> Mesh {
    match kind {
        GuideKind::Plane { width, height } => plane_mesh(*width, *height),
        GuideKind::Box { size } => box_mesh(*size),
        GuideKind::Sphere {
            radius,
            segments,
            rings,
        } => sphere_mesh(*radius, *segments, *rings),
        GuideKind::Cylinder {
            radius,
            height,
            segments,
        } => cylinder_mesh(*radius, *height, *segments),
        GuideKind::Cone {
            radius,
            height,
            segments,
        } => cone_mesh(*radius, *height, *segments),
        GuideKind::Torus {
            major_radius,
            minor_radius,
            major_segments,
            minor_segments,
        } => torus_mesh(
            *major_radius,
            *minor_radius,
            *major_segments,
            *minor_segments,
        ),
        GuideKind::Freeform { rows, subdivisions } => {
            let mut mesh = grid_mesh(rows, 1);
            if *subdivisions > 1 {
                mesh = grid_mesh(rows, *subdivisions);
            }
            mesh.compute_smooth_normals();
            mesh
        }
        GuideKind::Loft { rows, .. } => {
            let mut mesh = loft_mesh(rows);
            mesh.compute_smooth_normals();
            mesh
        }
        GuideKind::Imported { mesh, .. } => {
            let mut mesh = mesh.clone();
            if mesh.normals.len() != mesh.positions.len() {
                mesh.compute_smooth_normals();
            }
            mesh
        }
    }
}

fn plane_mesh(width: f32, height: f32) -> Mesh {
    let mut m = Mesh::new();
    let hw = width * 0.5;
    let hh = height * 0.5;
    let a = m.add_vertex(Vec3::new(-hw, 0.0, -hh), Vec3::Y, Vec2::ZERO);
    let b = m.add_vertex(Vec3::new(hw, 0.0, -hh), Vec3::Y, Vec2::X);
    let c = m.add_vertex(Vec3::new(hw, 0.0, hh), Vec3::Y, Vec2::ONE);
    let d = m.add_vertex(Vec3::new(-hw, 0.0, hh), Vec3::Y, Vec2::Y);
    m.push_triangle(a, b, c);
    m.push_triangle(a, c, d);
    m
}

fn box_mesh(size: Vec3) -> Mesh {
    let mut m = Mesh::new();
    let h = size.abs() * 0.5 + Vec3::splat(1.0e-5);
    let faces = [
        (Vec3::Y, Vec3::X, Vec3::Z),
        (-Vec3::Y, -Vec3::X, Vec3::Z),
        (Vec3::X, -Vec3::Z, Vec3::Y),
        (-Vec3::X, Vec3::Z, Vec3::Y),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (-Vec3::Z, -Vec3::X, Vec3::Y),
    ];
    for (normal, u_axis, v_axis) in faces {
        let centre = normal * h;
        let corners = [
            centre - u_axis * h - v_axis * h,
            centre + u_axis * h - v_axis * h,
            centre + u_axis * h + v_axis * h,
            centre - u_axis * h + v_axis * h,
        ];
        let mut ids = [0u32; 4];
        for (i, p) in corners.iter().enumerate() {
            let uv = match i {
                0 => Vec2::ZERO,
                1 => Vec2::X,
                2 => Vec2::ONE,
                _ => Vec2::Y,
            };
            ids[i] = m.add_vertex(*p, normal, uv);
        }
        m.push_triangle(ids[0], ids[1], ids[2]);
        m.push_triangle(ids[0], ids[2], ids[3]);
    }
    m
}

fn sphere_mesh(radius: f32, segments: u32, rings: u32) -> Mesh {
    let mut m = Mesh::new();
    let segments = segments.clamp(3, 256);
    let rings = rings.clamp(2, 256);
    for i in 0..=rings {
        let phi = PI * i as f32 / rings as f32;
        for j in 0..=segments {
            let theta = TAU * j as f32 / segments as f32;
            let p = Vec3::new(
                radius * phi.sin() * theta.cos(),
                radius * phi.cos(),
                radius * phi.sin() * theta.sin(),
            );
            let normal = if radius.abs() > 1.0e-6 {
                p / radius
            } else {
                Vec3::Y
            };
            m.add_vertex(
                p,
                normal,
                Vec2::new(j as f32 / segments as f32, i as f32 / rings as f32),
            );
        }
    }
    let row = segments + 1;
    for i in 0..rings {
        for j in 0..segments {
            let a = i * row + j;
            let b = a + 1;
            let c = a + row;
            let d = c + 1;
            m.push_triangle(a, c, b);
            m.push_triangle(b, c, d);
        }
    }
    m
}

fn cylinder_mesh(radius: f32, height: f32, segments: u32) -> Mesh {
    let mut m = Mesh::new();
    let segments = segments.clamp(3, 256);
    let hh = height * 0.5;
    for j in 0..=segments {
        let theta = TAU * j as f32 / segments as f32;
        let (s, c) = theta.sin_cos();
        let n = Vec3::new(c, 0.0, s);
        let bottom = m.add_vertex(
            Vec3::new(radius * c, -hh, radius * s),
            n,
            Vec2::new(j as f32 / segments as f32, 0.0),
        );
        let top = m.add_vertex(
            Vec3::new(radius * c, hh, radius * s),
            n,
            Vec2::new(j as f32 / segments as f32, 1.0),
        );
        if j > 0 {
            let prev_bottom = bottom - 2;
            let prev_top = top - 2;
            m.push_triangle(prev_bottom, bottom, prev_top);
            m.push_triangle(prev_top, bottom, top);
        }
    }
    // caps
    let bottom_centre = m.add_vertex(Vec3::new(0.0, -hh, 0.0), -Vec3::Y, Vec2::splat(0.5));
    let top_centre = m.add_vertex(Vec3::new(0.0, hh, 0.0), Vec3::Y, Vec2::splat(0.5));
    let cap_start = m.positions.len() as u32;
    for j in 0..segments {
        let theta = TAU * j as f32 / segments as f32;
        let c = theta.cos();
        let s = theta.sin();
        m.add_vertex(
            Vec3::new(radius * c, -hh, radius * s),
            -Vec3::Y,
            Vec2::new(c * 0.5 + 0.5, s * 0.5 + 0.5),
        );
        m.add_vertex(
            Vec3::new(radius * c, hh, radius * s),
            Vec3::Y,
            Vec2::new(c * 0.5 + 0.5, s * 0.5 + 0.5),
        );
    }
    for j in 0..segments {
        let j2 = (j + 1) % segments;
        let b0 = cap_start + j * 2;
        let t0 = b0 + 1;
        let b1 = cap_start + j2 * 2;
        let t1 = b1 + 1;
        m.push_triangle(bottom_centre, b1, b0);
        m.push_triangle(top_centre, t0, t1);
    }
    m
}

fn cone_mesh(radius: f32, height: f32, segments: u32) -> Mesh {
    let mut m = Mesh::new();
    let segments = segments.clamp(3, 256);
    let hh = height * 0.5;
    let slope = radius / height.max(1.0e-5);
    let apex = m.add_vertex(Vec3::new(0.0, hh, 0.0), Vec3::Y, Vec2::splat(0.5));
    let mut ring = Vec::new();
    for j in 0..=segments {
        let theta = TAU * j as f32 / segments as f32;
        let (s, c) = theta.sin_cos();
        let n = Vec3::new(c, slope, s).normalize();
        ring.push(m.add_vertex(
            Vec3::new(radius * c, -hh, radius * s),
            n,
            Vec2::new(j as f32 / segments as f32, 0.0),
        ));
    }
    for j in 0..segments as usize {
        m.push_triangle(apex, ring[j], ring[j + 1]);
    }
    let base = m.add_vertex(Vec3::new(0.0, -hh, 0.0), -Vec3::Y, Vec2::splat(0.5));
    let start = m.positions.len() as u32;
    for j in 0..segments {
        let theta = TAU * j as f32 / segments as f32;
        let c = theta.cos();
        let s = theta.sin();
        m.add_vertex(
            Vec3::new(radius * c, -hh, radius * s),
            -Vec3::Y,
            Vec2::new(c * 0.5 + 0.5, s * 0.5 + 0.5),
        );
    }
    for j in 0..segments {
        let j2 = (j + 1) % segments;
        m.push_triangle(base, start + j, start + j2);
    }
    m
}

fn torus_mesh(major: f32, minor: f32, major_segments: u32, minor_segments: u32) -> Mesh {
    let mut m = Mesh::new();
    let major_segments = major_segments.clamp(3, 256);
    let minor_segments = minor_segments.clamp(3, 128);
    for i in 0..=major_segments {
        let u = TAU * i as f32 / major_segments as f32;
        let (su, cu) = u.sin_cos();
        for j in 0..=minor_segments {
            let v = TAU * j as f32 / minor_segments as f32;
            let (sv, cv) = v.sin_cos();
            let p = Vec3::new(
                (major + minor * cv) * cu,
                minor * sv,
                (major + minor * cv) * su,
            );
            let n = Vec3::new(cv * cu, sv, cv * su);
            m.add_vertex(
                p,
                n,
                Vec2::new(
                    i as f32 / major_segments as f32,
                    j as f32 / minor_segments as f32,
                ),
            );
        }
    }
    let row = minor_segments + 1;
    for i in 0..major_segments {
        for j in 0..minor_segments {
            let a = i * row + j;
            let b = a + 1;
            let c = a + row;
            let d = c + 1;
            m.push_triangle(a, c, b);
            m.push_triangle(b, c, d);
        }
    }
    m
}

/// Build a grid surface from rows of points, subdividing each cell with simple
/// bilinear refinement (normals are smoothed afterwards).
fn grid_mesh(rows: &[Vec<Vec3>], subdivisions: u32) -> Mesh {
    let mut m = Mesh::new();
    if rows.len() < 2 {
        return m;
    }
    let cols = rows.iter().map(Vec::len).min().unwrap_or(0);
    if cols < 2 {
        return m;
    }
    let sub = subdivisions.max(1) as usize;
    for r in 0..rows.len() - 1 {
        for c in 0..cols - 1 {
            let p00 = rows[r][c];
            let p10 = rows[r][c + 1];
            let p01 = rows[r + 1][c];
            let p11 = rows[r + 1][c + 1];
            let base = m.positions.len() as u32;
            for sy in 0..=sub {
                for sx in 0..=sub {
                    let u = sx as f32 / sub as f32;
                    let v = sy as f32 / sub as f32;
                    let top = p00.lerp(p10, u);
                    let bottom = p01.lerp(p11, u);
                    let p = top.lerp(bottom, v);
                    m.add_vertex(
                        p,
                        Vec3::Y,
                        Vec2::new(
                            (c as f32 + u) / (cols - 1) as f32,
                            (r as f32 + v) / (rows.len() - 1) as f32,
                        ),
                    );
                }
            }
            let stride = sub + 1;
            for sy in 0..sub {
                for sx in 0..sub {
                    let i0 = base + (sy * stride + sx) as u32;
                    let i1 = i0 + 1;
                    let i2 = i0 + stride as u32;
                    let i3 = i2 + 1;
                    m.push_triangle(i0, i1, i2);
                    m.push_triangle(i1, i3, i2);
                }
            }
        }
    }
    m.compute_smooth_normals();
    m
}

fn loft_mesh(rows: &[Vec<Vec3>]) -> Mesh {
    grid_mesh(rows, 1)
}

/// Create rows for a ruled loft between two point lists, resampling the second
/// list to the same number of cross-sections. More than two curves interpolate
/// in sequence (a loft through all of them).
pub fn loft_rows(curves: &[Vec<Vec3>], samples: usize) -> Vec<Vec<Vec3>> {
    if curves.len() < 2 {
        return Vec::new();
    }
    let samples = samples.max(2);
    let resample = |pts: &[Vec3]| -> Vec<Vec3> {
        if pts.len() < 2 {
            return vec![pts.first().copied().unwrap_or(Vec3::ZERO); samples];
        }
        let mut out = Vec::with_capacity(samples);
        let total: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        if total < 1.0e-6 {
            return vec![pts[0]; samples];
        }
        for i in 0..samples {
            let target = total * i as f32 / (samples - 1) as f32;
            let mut acc = 0.0;
            let mut placed = false;
            for w in pts.windows(2) {
                let l = (w[1] - w[0]).length();
                if target <= acc + l {
                    let t = ((target - acc) / l.max(1.0e-6)).clamp(0.0, 1.0);
                    out.push(w[0].lerp(w[1], t));
                    placed = true;
                    break;
                }
                acc += l;
            }
            if !placed {
                out.push(*pts.last().unwrap());
            }
        }
        out
    };
    curves.iter().map(|c| resample(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::Ray;

    #[test]
    fn plane_hit_normal() {
        let mut g = Guide::plane(1, Vec3::new(0.0, 2.0, 0.0), Vec3::Y);
        let ray = Ray {
            origin: Vec3::new(0.5, 10.0, 0.5),
            dir: -Vec3::Y,
        };
        let hit = g.raycast(&ray).unwrap();
        assert!((hit.point.y - 2.0).abs() < 1e-4);
        assert!(hit.normal.y > 0.99);
        assert!((hit.distance - 8.0).abs() < 1e-4);
    }

    #[test]
    fn sphere_hit_radius() {
        let mut g = Guide::sphere(2, Vec3::ZERO, 5.0);
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 20.0),
            dir: -Vec3::Z,
        };
        let hit = g.raycast(&ray).unwrap();
        assert!((hit.point.length() - 5.0).abs() < 0.05);
    }

    #[test]
    fn transformed_sphere_still_hits() {
        let mut g = Guide::sphere(3, Vec3::ZERO, 2.0);
        g.transform =
            Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)) * Mat4::from_scale(Vec3::splat(2.0));
        let ray = Ray {
            origin: Vec3::new(10.0, 0.0, 20.0),
            dir: -Vec3::Z,
        };
        let hit = g.raycast(&ray).unwrap();
        assert!((hit.point.distance(Vec3::new(10.0, 0.0, 0.0)) - 4.0).abs() < 0.15);
    }

    #[test]
    fn cylinder_side_and_cap() {
        let mut g = Guide::cylinder(4, Vec3::ZERO, 2.0, 10.0);
        let side = Ray {
            origin: Vec3::new(10.0, 0.0, 0.0),
            dir: -Vec3::X,
        };
        let hit = g.raycast(&side).unwrap();
        assert!((hit.point.x - 2.0).abs() < 0.1);
        let cap = Ray {
            origin: Vec3::new(0.0, 20.0, 0.0),
            dir: -Vec3::Y,
        };
        let hit = g.raycast(&cap).unwrap();
        assert!((hit.point.y - 5.0).abs() < 0.1);
        assert!(hit.normal.y > 0.6);
    }

    #[test]
    fn cone_and_torus_build() {
        let cone = build_guide_mesh(&GuideKind::Cone {
            radius: 2.0,
            height: 5.0,
            segments: 16,
        });
        assert!(cone.triangle_count() > 16);
        cone.validate().unwrap();
        let torus = build_guide_mesh(&GuideKind::Torus {
            major_radius: 5.0,
            minor_radius: 1.0,
            major_segments: 24,
            minor_segments: 12,
        });
        assert!(torus.triangle_count() > 200);
        torus.validate().unwrap();
    }

    #[test]
    fn freeform_rows_create_triangles() {
        let rows = vec![
            vec![Vec3::ZERO, Vec3::X * 5.0, Vec3::X * 10.0],
            vec![
                Vec3::new(0.0, 5.0, 0.0),
                Vec3::new(5.0, 6.0, 0.0),
                Vec3::new(10.0, 5.0, 0.0),
            ],
        ];
        let mut g = Guide::freeform(5, rows, 2);
        assert!(g.mesh().triangle_count() >= 8);
        // The sheet lies in the z = 0 plane, so approach it at an angle.
        let ray = Ray {
            origin: Vec3::new(4.0, 30.0, 5.0),
            dir: Vec3::new(0.0, -1.0, -0.2).normalize(),
        };
        let hit = g.raycast(&ray).expect("freeform sheet should be hit");
        assert!(hit.point.y > 4.5 && hit.point.y < 5.5);
        assert!(hit.normal.z.abs() > 0.9);
    }

    #[test]
    fn loft_rows_are_resampled() {
        let a = vec![Vec3::ZERO, Vec3::X * 10.0];
        let b = vec![Vec3::new(0.0, 5.0, 0.0), Vec3::new(10.0, 5.0, 5.0)];
        let rows = loft_rows(&[a, b], 24);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.len() == 24));
        let mesh = loft_mesh(&rows);
        assert!(mesh.triangle_count() > 30);
    }

    #[test]
    fn inactive_guide_not_picked() {
        let mut guides = vec![Guide::sphere(6, Vec3::ZERO, 2.0)];
        guides[0].active = false;
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 10.0),
            dir: -Vec3::Z,
        };
        assert!(raycast_active(&mut guides, &ray).is_none());
        guides[0].active = true;
        assert!(raycast_active(&mut guides, &ray).is_some());
    }

    #[test]
    fn all_primitives_validate() {
        for kind in [
            GuideKind::Plane {
                width: 2.0,
                height: 2.0,
            },
            GuideKind::Box { size: Vec3::ONE },
            GuideKind::Sphere {
                radius: 1.0,
                segments: 12,
                rings: 8,
            },
            GuideKind::Cylinder {
                radius: 1.0,
                height: 2.0,
                segments: 12,
            },
            GuideKind::Cone {
                radius: 1.0,
                height: 2.0,
                segments: 12,
            },
            GuideKind::Torus {
                major_radius: 2.0,
                minor_radius: 0.5,
                major_segments: 12,
                minor_segments: 8,
            },
        ] {
            let mesh = build_guide_mesh(&kind);
            mesh.validate().unwrap();
        }
    }
}
