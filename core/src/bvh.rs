//! Axis-aligned bounding volume hierarchy for guide-surface ray casting.
//!
//! Built once per guide edit, then queried for every stylus sample. The build
//! uses median splits on the longest centroid axis; leaves hold at most eight
//! triangles. Ray traversal is iterative and allocation-free.

use crate::mesh::Mesh;
use crate::viewport::Ray;
use crate::{Vec2, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::empty()
    }
}

impl Aabb {
    pub fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Self {
        let mut b = Self::empty();
        for p in points {
            b.expand_point(p);
        }
        b
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn expand_point(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn expand(&mut self, other: Aabb) {
        if !other.is_empty() {
            self.min = self.min.min(other.min);
            self.max = self.max.max(other.max);
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn extent(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn surface_area(&self) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let e = self.extent();
        2.0 * (e.x * e.y + e.y * e.z + e.z * e.x)
    }

    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    pub fn expanded(&self, amount: f32) -> Aabb {
        Aabb {
            min: self.min - Vec3::splat(amount),
            max: self.max + Vec3::splat(amount),
        }
    }

    /// Slab test. Returns (t_enter, t_exit) with t_enter clamped to >= 0.
    pub fn ray_intersection(&self, ray: &Ray) -> Option<(f32, f32)> {
        if self.is_empty() {
            return None;
        }
        let mut tmin = 0.0f32;
        let mut tmax = f32::INFINITY;
        for axis in 0..3 {
            let o = ray.origin[axis];
            let d = ray.dir[axis];
            let lo = self.min[axis];
            let hi = self.max[axis];
            if d.abs() < 1.0e-12 {
                if o < lo || o > hi {
                    return None;
                }
            } else {
                let inv = 1.0 / d;
                let mut t1 = (lo - o) * inv;
                let mut t2 = (hi - o) * inv;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                tmin = tmin.max(t1);
                tmax = tmax.min(t2);
                if tmin > tmax {
                    return None;
                }
            }
        }
        Some((tmin, tmax))
    }
}

#[derive(Clone, Copy, Debug)]
struct Tri {
    positions: [Vec3; 3],
    normals: [Vec3; 3],
    uvs: [Vec2; 3],
    face_normal: Vec3,
    // Centroid and bounds cached for build and traversal.
    centroid: Vec3,
    bounds: Aabb,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb,
    left: u32,
    right: u32,
    start: u32,
    count: u32,
}

impl Node {
    fn is_leaf(&self) -> bool {
        self.count > 0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub triangle: u32,
    pub uv: Vec2,
    pub barycentric: Vec3,
}

#[derive(Clone, Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    triangles: Vec<Tri>,
    /// Sorted triangle indices. Leaves reference ranges here.
    order: Vec<u32>,
    bounds: Aabb,
    depth: u32,
}

impl Bvh {
    pub fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            triangles: Vec::new(),
            order: Vec::new(),
            bounds: Aabb::empty(),
            depth: 0,
        }
    }

    pub fn build(mesh: &Mesh) -> Self {
        if mesh.indices.len() < 3 || mesh.positions.is_empty() {
            return Self::empty();
        }
        let tri_count = mesh.indices.len() / 3;
        let has_normals = mesh.normals.len() == mesh.positions.len();
        let has_uvs = mesh.uvs.len() == mesh.positions.len();
        let mut triangles = Vec::with_capacity(tri_count);
        for i in 0..tri_count {
            let ia = mesh.indices[i * 3] as usize;
            let ib = mesh.indices[i * 3 + 1] as usize;
            let ic = mesh.indices[i * 3 + 2] as usize;
            if ia >= mesh.positions.len()
                || ib >= mesh.positions.len()
                || ic >= mesh.positions.len()
            {
                continue;
            }
            let positions = [mesh.positions[ia], mesh.positions[ib], mesh.positions[ic]];
            let normals = if has_normals {
                [mesh.normals[ia], mesh.normals[ib], mesh.normals[ic]]
            } else {
                [Vec3::Y; 3]
            };
            let uvs = if has_uvs {
                [mesh.uvs[ia], mesh.uvs[ib], mesh.uvs[ic]]
            } else {
                [Vec2::ZERO; 3]
            };
            let centroid = (positions[0] + positions[1] + positions[2]) / 3.0;
            let bounds = Aabb::from_points(positions);
            let face_normal = (positions[1] - positions[0])
                .cross(positions[2] - positions[0])
                .normalize_or_zero();
            triangles.push(Tri {
                positions,
                normals,
                uvs,
                face_normal,
                centroid,
                bounds,
            });
        }
        if triangles.is_empty() {
            return Self::empty();
        }
        let n = triangles.len();
        let mut bvh = Bvh {
            nodes: Vec::with_capacity(n * 2),
            triangles,
            order: (0..n as u32).collect(),
            bounds: Aabb::empty(),
            depth: 0,
        };
        let root_bounds = bvh.bounds_for_range(0, n);
        bvh.bounds = root_bounds;
        let root = bvh.build_node(0, n, 0);
        debug_assert_eq!(root, 0);
        bvh
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    fn bounds_for_range(&self, start: usize, end: usize) -> Aabb {
        let mut b = Aabb::empty();
        for i in start..end {
            b.expand(self.triangles[self.order[i] as usize].bounds);
        }
        b
    }

    fn centroid_bounds(&self, start: usize, end: usize) -> Aabb {
        let mut b = Aabb::empty();
        for i in start..end {
            b.expand_point(self.triangles[self.order[i] as usize].centroid);
        }
        b
    }

    fn build_node(&mut self, start: usize, end: usize, depth: u32) -> u32 {
        self.depth = self.depth.max(depth);
        let bounds = self.bounds_for_range(start, end);
        let node_index = self.nodes.len() as u32;
        self.nodes.push(Node {
            bounds,
            left: u32::MAX,
            right: u32::MAX,
            start: start as u32,
            count: (end - start) as u32,
        });
        let count = end - start;
        if count <= 8 || depth >= 48 {
            return node_index;
        }
        let centroid_bounds = self.centroid_bounds(start, end);
        let extents = centroid_bounds.extent();
        let axis = if extents.x >= extents.y && extents.x >= extents.z {
            0
        } else if extents.y >= extents.z {
            1
        } else {
            2
        };
        if extents[axis] < 1.0e-6 {
            return node_index;
        }
        let mid = start + count / 2;
        self.order[start..end].sort_unstable_by(|a, b| {
            self.triangles[*a as usize].centroid[axis]
                .partial_cmp(&self.triangles[*b as usize].centroid[axis])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let left = self.build_node(start, mid, depth + 1);
        let right = self.build_node(mid, end, depth + 1);
        self.nodes[node_index as usize].left = left;
        self.nodes[node_index as usize].right = right;
        self.nodes[node_index as usize].count = 0;
        node_index
    }

    pub fn raycast(&self, ray: &Ray) -> Option<RayHit> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut closest = f32::INFINITY;
        let mut hit: Option<RayHit> = None;
        let mut stack = [0u32; 128];
        let mut sp = 0usize;
        stack[sp] = 0;
        sp += 1;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if let Some((tmin, _)) = node.bounds.ray_intersection(ray) {
                if tmin > closest {
                    continue;
                }
            } else {
                continue;
            }
            if node.is_leaf() {
                let start = node.start as usize;
                let end = start + node.count as usize;
                for pos in start..end {
                    let tri_index = self.order[pos];
                    let tri = &self.triangles[tri_index as usize];
                    if let Some((t, u, v)) = ray_triangle(
                        ray.origin,
                        ray.dir,
                        tri.positions[0],
                        tri.positions[1],
                        tri.positions[2],
                    ) {
                        if t > 1.0e-5 && t < closest {
                            closest = t;
                            let w = 1.0 - u - v;
                            let mut normal =
                                tri.normals[0] * w + tri.normals[1] * u + tri.normals[2] * v;
                            if normal.length_squared() < 1.0e-12 {
                                normal = tri.face_normal;
                            }
                            normal = normal.normalize_or_zero();
                            if normal.dot(ray.dir) > 0.0 && tri.face_normal.dot(ray.dir) < 0.0 {
                                normal = -normal;
                            }
                            hit = Some(RayHit {
                                distance: t,
                                point: ray.at(t),
                                normal,
                                triangle: tri_index,
                                uv: tri.uvs[0] * w + tri.uvs[1] * u + tri.uvs[2] * v,
                                barycentric: Vec3::new(w, u, v),
                            });
                        }
                    }
                }
            } else {
                if sp + 2 <= stack.len() {
                    stack[sp] = node.left;
                    sp += 1;
                    stack[sp] = node.right;
                    sp += 1;
                }
            }
        }
        hit
    }
}

/// Möller–Trumbore ray/triangle intersection. Returns (distance, u, v).
pub fn ray_triangle(
    origin: Vec3,
    dir: Vec3,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
) -> Option<(f32, f32, f32)> {
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = dir.cross(edge2);
    let a = edge1.dot(h);
    if a.abs() < 1.0e-9 {
        return None;
    }
    let f = 1.0 / a;
    let s = origin - v0;
    let u = f * s.dot(h);
    if !(-1.0e-6..=1.0 + 1.0e-6).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = f * dir.dot(q);
    if v < -1.0e-6 || u + v > 1.0 + 1.0e-6 {
        return None;
    }
    let t = f * edge2.dot(q);
    if t > 1.0e-6 {
        Some((t, u, v))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn plane_mesh(extent: f32) -> Mesh {
        let mut m = Mesh::new();
        let a = m.add_vertex(Vec3::new(-extent, 0.0, -extent), Vec3::Y, Vec2::ZERO);
        let b = m.add_vertex(Vec3::new(extent, 0.0, -extent), Vec3::Y, Vec2::X);
        let c = m.add_vertex(Vec3::new(extent, 0.0, extent), Vec3::Y, Vec2::ONE);
        let d = m.add_vertex(Vec3::new(-extent, 0.0, extent), Vec3::Y, Vec2::Y);
        m.push_triangle(a, b, c);
        m.push_triangle(a, c, d);
        m
    }

    fn sphere_mesh(radius: f32, stacks: usize, slices: usize) -> Mesh {
        let mut m = Mesh::new();
        for i in 0..=stacks {
            let phi = std::f32::consts::PI * i as f32 / stacks as f32;
            for j in 0..=slices {
                let theta = std::f32::consts::TAU * j as f32 / slices as f32;
                let p = Vec3::new(
                    radius * phi.sin() * theta.cos(),
                    radius * phi.cos(),
                    radius * phi.sin() * theta.sin(),
                );
                m.add_vertex(
                    p,
                    p.normalize(),
                    Vec2::new(j as f32 / slices as f32, i as f32 / stacks as f32),
                );
            }
        }
        let row = slices + 1;
        for i in 0..stacks {
            for j in 0..slices {
                let a = (i * row + j) as u32;
                let b = a + 1;
                let c = a + row as u32;
                let d = c + 1;
                m.push_triangle(a, c, b);
                m.push_triangle(b, c, d);
            }
        }
        m
    }

    #[test]
    fn ray_hits_plane() {
        let bvh = Bvh::build(&plane_mesh(10.0));
        let ray = Ray {
            origin: Vec3::new(1.0, 5.0, 1.0),
            dir: -Vec3::Y,
        };
        let hit = bvh.raycast(&ray).unwrap();
        assert!((hit.point.y).abs() < 1e-5);
        assert!((hit.distance - 5.0).abs() < 1e-5);
        assert!(hit.normal.y.abs() > 0.99);
    }

    #[test]
    fn ray_hits_sphere() {
        let bvh = Bvh::build(&sphere_mesh(2.0, 48, 96));
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 10.0),
            dir: -Vec3::Z,
        };
        let hit = bvh.raycast(&ray).unwrap();
        assert!((hit.distance - 8.0).abs() < 0.1);
        assert!((hit.point.length() - 2.0).abs() < 0.05);
        assert!(hit.normal.dot(Vec3::Z) > 0.5);
    }

    #[test]
    fn miss_returns_none() {
        let bvh = Bvh::build(&sphere_mesh(1.0, 16, 32));
        let ray = Ray {
            origin: Vec3::new(10.0, 10.0, 10.0),
            dir: Vec3::Y,
        };
        assert!(bvh.raycast(&ray).is_none());
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 10.0),
            dir: Vec3::Z,
        };
        assert!(bvh.raycast(&ray).is_none());
    }

    #[test]
    fn aabb_slab() {
        let b = Aabb::new(-Vec3::ONE, Vec3::ONE);
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 5.0),
            dir: -Vec3::Z,
        };
        let (a, b2) = b.ray_intersection(&ray).unwrap();
        assert!((a - 4.0).abs() < 1e-5);
        assert!((b2 - 6.0).abs() < 1e-5);
        let ray = Ray {
            origin: Vec3::new(0.0, 3.0, 5.0),
            dir: -Vec3::Z,
        };
        assert!(b.ray_intersection(&ray).is_none());
    }

    #[test]
    fn ten_thousand_triangles_fast() {
        let mesh = sphere_mesh(5.0, 48, 110);
        assert!(mesh.triangle_count() > 10_000);
        let start = Instant::now();
        let bvh = Bvh::build(&mesh);
        let build_ms = start.elapsed().as_secs_f32() * 1000.0;
        assert!(build_ms < 100.0, "build took {build_ms} ms");
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 20.0),
            dir: -Vec3::Z,
        };
        let start = Instant::now();
        for _ in 0..100 {
            assert!(bvh.raycast(&ray).is_some());
        }
        let avg_us = start.elapsed().as_secs_f64() * 1.0e6 / 100.0;
        assert!(avg_us < 100.0, "raycast {avg_us} us");
        assert!(bvh.depth() < 48);
    }

    #[test]
    fn leaf_size_respected() {
        let bvh = Bvh::build(&sphere_mesh(1.0, 20, 40));
        for node in &bvh.nodes {
            assert!(node.count <= 8);
        }
    }

    #[test]
    fn empty_mesh_is_safe() {
        let bvh = Bvh::build(&Mesh::new());
        assert!(bvh.is_empty());
        assert!(bvh
            .raycast(&Ray {
                origin: Vec3::ZERO,
                dir: Vec3::Z
            })
            .is_none());
    }
}
