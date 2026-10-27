//! CPU mesh container and optimisation utilities.

use crate::{Mat3, Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// An indexed triangle mesh with smooth normals and UV coordinates.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() || self.indices.is_empty()
    }

    pub fn add_vertex(&mut self, position: Vec3, normal: Vec3, uv: Vec2) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(position);
        self.normals.push(normal);
        self.uvs.push(uv);
        index
    }

    pub fn push_triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    pub fn append(&mut self, other: &Mesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    pub fn bounds(&self) -> (Vec3, Vec3) {
        if self.positions.is_empty() {
            return (Vec3::ZERO, Vec3::ZERO);
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.positions {
            min = min.min(*p);
            max = max.max(*p);
        }
        (min, max)
    }

    /// Area-weighted smooth normals.
    pub fn compute_smooth_normals(&mut self) {
        self.normals = vec![Vec3::ZERO; self.positions.len()];
        for tri in self.indices.as_chunks::<3>().0 {
            let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            if a >= self.positions.len() || b >= self.positions.len() || c >= self.positions.len() {
                continue;
            }
            let face = (self.positions[b] - self.positions[a])
                .cross(self.positions[c] - self.positions[a]);
            if face.length_squared() > 0.0 && face.is_finite() {
                self.normals[a] += face;
                self.normals[b] += face;
                self.normals[c] += face;
            }
        }
        for n in &mut self.normals {
            *n = n.normalize_or_zero();
            if !n.is_finite() {
                *n = Vec3::Y;
            }
        }
    }

    pub fn face_normal(&self, triangle: usize) -> Vec3 {
        let base = triangle * 3;
        if base + 2 >= self.indices.len() {
            return Vec3::Y;
        }
        let a = self.positions[self.indices[base] as usize];
        let b = self.positions[self.indices[base + 1] as usize];
        let c = self.positions[self.indices[base + 2] as usize];
        (b - a).cross(c - a).normalize_or_zero()
    }

    pub fn transform(&mut self, mat: Mat4) {
        let nm = Mat3::from_mat4(mat).inverse().transpose();
        for p in &mut self.positions {
            *p = mat.transform_point3(*p);
        }
        for n in &mut self.normals {
            *n = (nm * *n).normalize_or_zero();
        }
    }

    /// Weld vertices within `tolerance` and return the removed vertex count.
    /// Normals and UVs are averaged. Degenerate triangles are dropped.
    pub fn weld(&mut self, tolerance: f32) -> usize {
        let tol = tolerance.max(1.0e-6);
        let inv = 1.0 / tol;
        let mut map: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut remap = vec![0u32; self.positions.len()];
        let mut new_positions = Vec::with_capacity(self.positions.len());
        let mut new_normals: Vec<Vec3> = Vec::with_capacity(self.positions.len());
        let mut new_uvs: Vec<Vec2> = Vec::with_capacity(self.positions.len());
        let mut counts: Vec<u32> = Vec::with_capacity(self.positions.len());
        let old_normals = if self.normals.len() == self.positions.len() {
            self.normals.clone()
        } else {
            vec![Vec3::ZERO; self.positions.len()]
        };
        let old_uvs = if self.uvs.len() == self.positions.len() {
            self.uvs.clone()
        } else {
            vec![Vec2::ZERO; self.positions.len()]
        };
        for (i, p) in self.positions.iter().enumerate() {
            let key = (
                (p.x * inv).round() as i64,
                (p.y * inv).round() as i64,
                (p.z * inv).round() as i64,
            );
            if let Some(&existing) = map.get(&key) {
                remap[i] = existing;
                let idx = existing as usize;
                new_normals[idx] += old_normals[i];
                new_uvs[idx] += old_uvs[i];
                counts[idx] += 1;
            } else {
                let idx = new_positions.len() as u32;
                map.insert(key, idx);
                remap[i] = idx;
                new_positions.push(*p);
                new_normals.push(old_normals[i]);
                new_uvs.push(old_uvs[i]);
                counts.push(1);
            }
        }
        for i in 0..new_positions.len() {
            let c = counts[i].max(1) as f32;
            new_normals[i] = (new_normals[i] / c).normalize_or_zero();
            new_uvs[i] /= c;
        }
        let mut new_indices = Vec::with_capacity(self.indices.len());
        for tri in self.indices.as_chunks::<3>().0 {
            let a = remap[tri[0] as usize];
            let b = remap[tri[1] as usize];
            let c = remap[tri[2] as usize];
            if a != b && b != c && a != c {
                new_indices.extend_from_slice(&[a, b, c]);
            }
        }
        let removed = self.positions.len().saturating_sub(new_positions.len());
        self.positions = new_positions;
        self.normals = new_normals;
        self.uvs = new_uvs;
        self.indices = new_indices;
        removed
    }

    /// Weld, smooth and validate. Returns a human-readable report.
    pub fn optimize(&mut self, tolerance: f32) -> MeshOptimizeReport {
        let before = self.positions.len();
        let welded = self.weld(tolerance);
        self.compute_smooth_normals();
        let invalid = self.positions.iter().any(|v| !v.is_finite())
            || self.normals.iter().any(|v| !v.is_finite())
            || self.uvs.iter().any(|v| !v.is_finite());
        let invalid = usize::from(invalid);
        MeshOptimizeReport {
            vertices_before: before,
            vertices_after: self.positions.len(),
            triangles: self.triangle_count(),
            welded_vertices: welded,
            invalid_values: invalid,
        }
    }

    /// Simple vertex-cluster decimation. Triangles whose three vertices fall in
    /// the same cluster collapse, and other vertices are remapped to cluster
    /// centres. Useful for viewport-only LOD and STL export.
    pub fn decimate_cluster(&mut self, cell: f32) {
        let cell = cell.max(1.0e-5);
        // A sorted map keeps vertex and index ordering deterministic across
        // runs and platforms, which matters for reproducible exports.
        let mut clusters: BTreeMap<(i64, i64, i64), Vec<u32>> = BTreeMap::new();
        for (i, p) in self.positions.iter().enumerate() {
            let key = (
                (p.x / cell).floor() as i64,
                (p.y / cell).floor() as i64,
                (p.z / cell).floor() as i64,
            );
            clusters.entry(key).or_default().push(i as u32);
        }
        let mut remap = vec![0u32; self.positions.len()];
        let mut new_positions = Vec::with_capacity(clusters.len());
        let mut new_uvs = Vec::with_capacity(clusters.len());
        let mut new_normals = Vec::with_capacity(clusters.len());
        for ids in clusters.values() {
            let idx = new_positions.len() as u32;
            let mut p = Vec3::ZERO;
            let mut uv = Vec2::ZERO;
            let mut n = Vec3::ZERO;
            for i in ids {
                let i = *i as usize;
                p += self.positions[i];
                if i < self.uvs.len() {
                    uv += self.uvs[i];
                }
                if i < self.normals.len() {
                    n += self.normals[i];
                }
                remap[i] = idx;
            }
            let c = ids.len() as f32;
            new_positions.push(p / c);
            new_uvs.push(uv / c);
            new_normals.push(n.normalize_or_zero());
        }
        let mut new_indices = Vec::new();
        for tri in self.indices.as_chunks::<3>().0 {
            let a = remap[tri[0] as usize];
            let b = remap[tri[1] as usize];
            let c = remap[tri[2] as usize];
            if a != b && b != c && a != c {
                new_indices.extend_from_slice(&[a, b, c]);
            }
        }
        self.positions = new_positions;
        self.uvs = new_uvs;
        self.normals = new_normals;
        self.indices = new_indices;
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.indices.len().is_multiple_of(3) {
            return Err("index count is not a multiple of 3".into());
        }
        for i in &self.indices {
            if *i as usize >= self.positions.len() {
                return Err(format!("index {i} out of range"));
            }
        }
        if !self.positions.iter().all(|p| p.is_finite()) {
            return Err("non-finite position".into());
        }
        if self.normals.len() == self.positions.len() && !self.normals.iter().all(|n| n.is_finite())
        {
            return Err("non-finite normal".into());
        }
        if !self.uvs.iter().all(|u| u.is_finite()) {
            return Err("non-finite uv".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MeshOptimizeReport {
    pub vertices_before: usize,
    pub vertices_after: usize,
    pub triangles: usize,
    pub welded_vertices: usize,
    pub invalid_values: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> Mesh {
        let mut m = Mesh::new();
        let a = m.add_vertex(Vec3::ZERO, Vec3::Y, Vec2::ZERO);
        let b = m.add_vertex(Vec3::X, Vec3::Y, Vec2::X);
        let c = m.add_vertex(Vec3::new(1.0, 0.0, 1.0), Vec3::Y, Vec2::new(1.0, 1.0));
        let d = m.add_vertex(Vec3::Z, Vec3::Y, Vec2::Y);
        m.push_triangle(a, c, b);
        m.push_triangle(a, d, c);
        m
    }

    #[test]
    fn normals_are_unit_length() {
        let mut m = quad();
        m.compute_smooth_normals();
        for n in &m.normals {
            assert!((n.length() - 1.0).abs() < 1e-5);
            assert!(n.y > 0.99);
        }
    }

    #[test]
    fn weld_removes_duplicates() {
        let mut m = Mesh::new();
        let a = m.add_vertex(Vec3::ZERO, Vec3::Y, Vec2::ZERO);
        let b = m.add_vertex(Vec3::X, Vec3::Y, Vec2::X);
        let c = m.add_vertex(Vec3::new(1.0, 0.0, 1.0), Vec3::Y, Vec2::new(1.0, 1.0));
        let a2 = m.add_vertex(Vec3::new(0.00001, 0.0, 0.0), Vec3::Y, Vec2::ZERO);
        m.push_triangle(a, b, c);
        m.push_triangle(a2, c, b);
        let before = m.vertex_count();
        let removed = m.weld(0.001);
        assert_eq!(removed, 1);
        assert_eq!(m.vertex_count(), before - 1);
        assert_eq!(m.triangle_count(), 2);
    }

    #[test]
    fn append_offsets_indices() {
        let mut a = quad();
        let b = quad();
        a.append(&b);
        assert_eq!(a.vertex_count(), 8);
        assert_eq!(a.triangle_count(), 4);
        assert!(a.indices.iter().all(|i| *i < 8));
    }

    #[test]
    fn decimate_reduces_vertices() {
        let mut m = Mesh::new();
        for x in 0..10 {
            for z in 0..10 {
                m.add_vertex(
                    Vec3::new(x as f32, 0.0, z as f32),
                    Vec3::Y,
                    Vec2::new(x as f32 / 10.0, z as f32 / 10.0),
                );
            }
        }
        for x in 0..9 {
            for z in 0..9 {
                let i = x + z * 10;
                m.push_triangle(i as u32, (i + 1) as u32, (i + 10) as u32);
                m.push_triangle((i + 1) as u32, (i + 11) as u32, (i + 10) as u32);
            }
        }
        let before = m.vertex_count();
        let mut cloned = m.clone();
        m.decimate_cluster(2.0);
        cloned.decimate_cluster(2.0);
        assert_eq!(m, cloned, "cluster decimation must be deterministic");
        assert!(m.vertex_count() < before);
        m.validate().unwrap();
    }

    #[test]
    fn transform_moves_normals() {
        let mut m = quad();
        m.transform(Mat4::from_rotation_x(std::f32::consts::PI));
        assert!(m.positions.iter().all(|p| p.is_finite()));
        assert!(m.normals.iter().any(|n| n.y < 0.0));
    }
}
