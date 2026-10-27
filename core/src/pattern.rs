//! Procedural patterns used by brushes.
//!
//! Patterns are evaluated in UV space but generated procedurally so they can
//! be re-evaluated at any resolution.  The same functions back the CPU
//! reference (unit tests and texture baking) and the WGSL shader.

use crate::math::{hash2, hash_u32, line_distance, smoothstep};
use crate::{Vec2, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind {
    None,
    Halftone,
    Hatch,
    CrossHatch,
    Stipple,
    Terrazzo,
    Custom,
}

impl PatternKind {
    pub fn label(self) -> &'static str {
        match self {
            PatternKind::None => "Solid",
            PatternKind::Halftone => "Halftone",
            PatternKind::Hatch => "Hatch",
            PatternKind::CrossHatch => "Cross-hatch",
            PatternKind::Stipple => "Stipple",
            PatternKind::Terrazzo => "Terrazzo",
            PatternKind::Custom => "Custom tile",
        }
    }

    /// Stable shader id. Kept in sync with `pattern.wgsl`.
    pub fn shader_id(self) -> u32 {
        match self {
            PatternKind::None => 0,
            PatternKind::Halftone => 1,
            PatternKind::Hatch => 2,
            PatternKind::CrossHatch => 3,
            PatternKind::Stipple => 4,
            PatternKind::Terrazzo => 5,
            PatternKind::Custom => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternChannel {
    /// Pattern holes remove opacity (default for cutout brushes).
    Alpha,
    /// Pattern modulates towards the secondary colour.
    Color,
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub kind: PatternKind,
    /// Repeat count across the stroke's U range.
    pub scale_u: f32,
    /// Repeat count around the cross-section.
    pub scale_v: f32,
    /// Pattern cell spacing in normalised pattern space.
    pub spacing: f32,
    /// Dot radius / line half-width in cell units (0..1).
    pub thickness: f32,
    pub angle_deg: f32,
    /// 0..1 random coverage for stipple / density for halftone.
    pub density: f32,
    /// How strongly pressure opens the pattern (0 = constant, 1 = pressure driven).
    pub pressure_reactive: f32,
    pub color_b: Vec4,
    pub channel: PatternChannel,
    /// Relative path inside a .plume archive for custom tiles.
    pub custom_tile: Option<String>,
    pub seed: u32,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            kind: PatternKind::None,
            scale_u: 8.0,
            scale_v: 3.0,
            spacing: 0.25,
            thickness: 0.16,
            angle_deg: 45.0,
            density: 0.55,
            pressure_reactive: 0.65,
            color_b: Vec4::new(0.0, 0.0, 0.0, 1.0),
            channel: PatternChannel::Alpha,
            custom_tile: None,
            seed: 1,
        }
    }
}

impl Pattern {
    pub fn solid() -> Self {
        Self::default()
    }

    pub fn halftone(spacing: f32, thickness: f32) -> Self {
        Self {
            kind: PatternKind::Halftone,
            spacing,
            thickness,
            scale_u: 12.0,
            scale_v: 4.0,
            density: 0.8,
            ..Self::default()
        }
    }

    pub fn hatch(angle_deg: f32, spacing: f32) -> Self {
        Self {
            kind: PatternKind::Hatch,
            angle_deg,
            spacing,
            thickness: 0.12,
            scale_u: 10.0,
            scale_v: 3.0,
            ..Self::default()
        }
    }

    pub fn cross_hatch(spacing: f32) -> Self {
        Self {
            kind: PatternKind::CrossHatch,
            angle_deg: 45.0,
            spacing,
            thickness: 0.1,
            scale_u: 10.0,
            scale_v: 3.0,
            ..Self::default()
        }
    }

    pub fn stipple(density: f32) -> Self {
        Self {
            kind: PatternKind::Stipple,
            density,
            spacing: 0.35,
            thickness: 0.14,
            scale_u: 14.0,
            scale_v: 4.0,
            ..Self::default()
        }
    }

    pub fn terrazzo(spacing: f32) -> Self {
        Self {
            kind: PatternKind::Terrazzo,
            spacing,
            thickness: 0.05,
            scale_u: 6.0,
            scale_v: 2.0,
            color_b: Vec4::new(0.95, 0.55, 0.22, 1.0),
            channel: PatternChannel::Color,
            ..Self::default()
        }
    }

    pub fn custom(path: impl Into<String>) -> Self {
        Self {
            kind: PatternKind::Custom,
            custom_tile: Some(path.into()),
            ..Self::default()
        }
    }

    /// Sample the pattern. `uv` need not be in 0..1; the pattern repeats.
    /// `pressure` is the interpolated stroke pressure at this point.
    pub fn sample(&self, uv: Vec2, pressure: f32) -> PatternSample {
        let pressure = pressure.clamp(0.0, 1.0);
        let drive = 1.0 - self.pressure_reactive + self.pressure_reactive * pressure;
        let p = Vec2::new(uv.x * self.scale_u, uv.y * self.scale_v);
        let angle = self.angle_deg.to_radians();
        let spacing = self.spacing.max(0.001);
        match self.kind {
            PatternKind::None => PatternSample {
                alpha: 1.0,
                color: Vec4::ONE,
            },
            PatternKind::Halftone => {
                let cell = p / spacing;
                let local = Vec2::new(cell.x.fract() - 0.5, cell.y.fract() - 0.5);
                let radius = self.thickness * (0.25 + 0.75 * drive);
                let d = local.length();
                let edge = smoothstep(radius + 0.04, radius - 0.04, d);
                PatternSample {
                    alpha: edge,
                    color: Vec4::ONE,
                }
            }
            PatternKind::Hatch => {
                let d = line_distance(p, angle, spacing);
                let width = self.thickness * spacing * (0.35 + 0.65 * drive);
                PatternSample {
                    alpha: smoothstep(width + 0.02, width - 0.02, d),
                    color: Vec4::ONE,
                }
            }
            PatternKind::CrossHatch => {
                let d1 = line_distance(p, angle, spacing);
                let d2 = line_distance(p, angle + std::f32::consts::FRAC_PI_2, spacing);
                let width = self.thickness * spacing * (0.35 + 0.65 * drive);
                let a = smoothstep(width + 0.02, width - 0.02, d1.min(d2));
                PatternSample {
                    alpha: a,
                    color: Vec4::ONE,
                }
            }
            PatternKind::Stipple => {
                let cell = p / spacing;
                let ix = cell.x.floor() as i32;
                let iy = cell.y.floor() as i32;
                let h = hash2(ix ^ self.seed as i32, iy);
                let keep = h < self.density * (0.35 + 0.65 * drive);
                let jitter = Vec2::new(
                    hash2(ix.wrapping_mul(3), iy.wrapping_mul(7)),
                    hash2(ix.wrapping_mul(11), iy.wrapping_mul(13)),
                );
                let local = Vec2::new(cell.x.fract(), cell.y.fract())
                    - (Vec2::splat(0.5) + (jitter - Vec2::splat(0.5)) * 0.5);
                let r = self.thickness * (0.5 + 0.5 * drive);
                PatternSample {
                    alpha: if keep {
                        smoothstep(r + 0.05, r - 0.05, local.length())
                    } else {
                        0.0
                    },
                    color: Vec4::ONE,
                }
            }
            PatternKind::Terrazzo => {
                let cell = p / spacing;
                let base = Vec2::new(cell.x.floor(), cell.y.floor());
                let mut best = f32::MAX;
                let mut second = f32::MAX;
                let mut id = 0u32;
                for oy in -1..=1 {
                    for ox in -1..=1 {
                        let gx = base.x as i32 + ox;
                        let gy = base.y as i32 + oy;
                        let h = hash_u32(
                            (gx as u32).wrapping_mul(0x9e37_79b9)
                                ^ (gy as u32).wrapping_mul(0x85eb_ca6b)
                                ^ self.seed,
                        );
                        let jx = ((h & 0xffff) as f32 / 65535.0 - 0.5) * 0.8;
                        let jy = (((h >> 16) & 0xffff) as f32 / 65535.0 - 0.5) * 0.8;
                        let feature = Vec2::new(gx as f32 + 0.5 + jx, gy as f32 + 0.5 + jy);
                        let d = (cell - feature).length();
                        if d < best {
                            second = best;
                            best = d;
                            id = h;
                        } else if d < second {
                            second = d;
                        }
                    }
                }
                let border = (second - best) / spacing;
                let inside = smoothstep(self.thickness, self.thickness + 0.06, border);
                let tint = (id & 0xff) as f32 / 255.0;
                let mix = Vec4::new(tint, (id >> 8 & 0xff) as f32 / 765.0, 0.0, 0.0);
                PatternSample {
                    alpha: inside,
                    color: Vec4::new(
                        self.color_b.x * (0.7 + 0.3 * tint),
                        self.color_b.y * (0.8 + 0.2 * mix.y),
                        self.color_b.z,
                        1.0,
                    ),
                }
            }
            PatternKind::Custom => PatternSample {
                alpha: 1.0,
                color: Vec4::ONE,
            },
        }
    }

    /// Number of approximate dots for a halftone pattern across the full
    /// 0..1 UV range. The pattern space spans `scale_u` x `scale_v` units and
    /// cells repeat every `spacing` units, so this is
    /// `floor(scale_u / spacing) * floor(scale_v / spacing)`.
    pub fn halftone_dot_capacity(&self) -> u32 {
        let u = (self.scale_u / self.spacing.max(0.001)).floor().max(0.0) as u32;
        let v = (self.scale_v / self.spacing.max(0.001)).floor().max(0.0) as u32;
        u.saturating_mul(v)
    }

    /// Bake this pattern to a seamless RGBA tile for export.
    pub fn bake_tile(&self, size: u32) -> image::RgbaImage {
        let size = size.clamp(16, 1024);
        let mut img = image::RgbaImage::new(size, size);
        for y in 0..size {
            for x in 0..size {
                let uv = Vec2::new(x as f32 / size as f32, y as f32 / size as f32);
                let s = self.sample(uv, 1.0);
                // Preserve the soft procedural edge instead of thresholding it
                // to a hard cut-out.
                let a = (s.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
                let c = if self.channel == PatternChannel::Color
                    || self.channel == PatternChannel::Both
                {
                    Vec4::new(s.color.x, s.color.y, s.color.z, 1.0)
                } else {
                    Vec4::ONE
                };
                img.put_pixel(
                    x,
                    y,
                    image::Rgba([
                        (c.x.clamp(0.0, 1.0) * 255.0) as u8,
                        (c.y.clamp(0.0, 1.0) * 255.0) as u8,
                        (c.z.clamp(0.0, 1.0) * 255.0) as u8,
                        a,
                    ]),
                );
            }
        }
        img
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternSample {
    pub alpha: f32,
    pub color: Vec4,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec2;

    #[test]
    fn solid_is_opaque() {
        let s = Pattern::solid().sample(Vec2::new(0.3, 0.5), 0.2);
        assert_eq!(s.alpha, 1.0);
    }

    #[test]
    fn halftone_has_dots_and_gaps() {
        let p = Pattern::halftone(0.25, 0.18);
        let centre = p.sample(Vec2::new(0.5 * 0.25 / 12.0, 0.5 * 0.25 / 4.0), 1.0);
        assert!(
            centre.alpha > 0.5,
            "cell centre should be a dot: {}",
            centre.alpha
        );
        let corner = p.sample(Vec2::new(0.01 / 12.0, 0.01 / 4.0), 1.0);
        assert!(
            corner.alpha < 0.5,
            "cell corner should be a gap: {}",
            corner.alpha
        );
    }

    #[test]
    fn halftone_dot_count_matches_capacity() {
        let p = Pattern::halftone(0.25, 0.18);
        let cells_u = (p.scale_u / p.spacing).floor() as u32;
        let cells_v = (p.scale_v / p.spacing).floor() as u32;
        assert_eq!(cells_u, 48);
        assert_eq!(cells_v, 16);
        assert_eq!(p.halftone_dot_capacity(), cells_u * cells_v);
        // Every cell centre across the full UV range must be a dot.
        let mut dots = 0;
        for j in 0..cells_v {
            for i in 0..cells_u {
                let uv = Vec2::new(
                    (i as f32 + 0.5) * p.spacing / p.scale_u,
                    (j as f32 + 0.5) * p.spacing / p.scale_v,
                );
                if p.sample(uv, 1.0).alpha > 0.5 {
                    dots += 1;
                }
            }
        }
        assert_eq!(dots, p.halftone_dot_capacity());
    }

    #[test]
    fn cross_hatch_has_two_families() {
        let p = Pattern::cross_hatch(0.2);
        // Walk along x at fixed y: at least two bright bands per repeat.
        let mut bands = 0;
        let mut inside = false;
        for i in 0..400 {
            let x = i as f32 / 400.0;
            let a = p.sample(Vec2::new(x, 0.31), 1.0).alpha > 0.25;
            if a && !inside {
                bands += 1;
            }
            inside = a;
        }
        assert!(bands >= 2, "expected multiple hatch bands, got {bands}");
    }

    #[test]
    fn stipple_coverage_is_in_density_ballpark() {
        let p = Pattern::stipple(0.5);
        let mut on = 0;
        let n = 100;
        for j in 0..n {
            for i in 0..n {
                let uv = Vec2::new(i as f32 / n as f32, j as f32 / n as f32);
                if p.sample(uv, 1.0).alpha > 0.5 {
                    on += 1;
                }
            }
        }
        let frac = on as f32 / (n * n) as f32;
        assert!(frac > 0.005 && frac < 0.6, "stipple coverage {frac}");
    }

    #[test]
    fn terrazzo_varies_colour() {
        let p = Pattern::terrazzo(0.3);
        let a = p.sample(Vec2::new(0.13, 0.24), 1.0);
        let b = p.sample(Vec2::new(0.61, 0.72), 1.0);
        assert!((a.color - b.color).length() > 0.001 || (a.alpha - b.alpha).abs() > 0.001);
    }

    #[test]
    fn bake_tile_keeps_soft_alpha_edges() {
        let img = Pattern::halftone(0.2, 0.2).bake_tile(64);
        assert!(
            img.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 255),
            "baked pattern should contain antialiased alpha values",
        );
    }

    #[test]
    fn bake_tile_has_content() {
        let img = Pattern::halftone(0.2, 0.2).bake_tile(64);
        assert_eq!(img.dimensions(), (64, 64));
        assert!(img.pixels().any(|p| p[3] > 0));
        assert!(img.pixels().any(|p| p[3] == 0));
    }
}
