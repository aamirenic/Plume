//! Curated, procedurally generated starting points.
//!
//! Gallery scenes are created from PLUME's own brushes and curves. They contain
//! no copied artwork, bitmaps or preset files from any other application.

use crate::brush::{Brush, BrushLibrary};
use crate::curve::{Curve, CurveKind};
use crate::guide::Guide;
use crate::render::{BackgroundKind, Environment};
use crate::scene::Document;
use crate::viewport::Camera;
use crate::{Vec2, Vec3, Vec4};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GalleryItem {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
}

pub const GALLERY: &[GalleryItem] = &[
    GalleryItem {
        id: "ribbon-bloom",
        name: "Ribbon Bloom",
        description: "Layered calligraphic ribbons in warm paper colours.",
        category: "Featured",
    },
    GalleryItem {
        id: "wire-flora",
        name: "Wire Flora",
        description: "A generative wire plant with circular petals.",
        category: "Nature",
    },
    GalleryItem {
        id: "paper-lanterns",
        name: "Paper Lanterns",
        description: "Floating elliptical lanterns on a soft studio stage.",
        category: "Product",
    },
    GalleryItem {
        id: "neon-knot",
        name: "Neon Knot",
        description: "A glowing torus knot built from Catmull-Rom strokes.",
        category: "Experimental",
    },
    GalleryItem {
        id: "terrazzo-tile",
        name: "Terrazzo Tile",
        description: "Flat patterned strokes exploring procedural material.",
        category: "Materials",
    },
    GalleryItem {
        id: "ink-orbit",
        name: "Ink Orbit",
        description: "Concentric orbits around a sphere guide.",
        category: "Featured",
    },
    GalleryItem {
        id: "calligraphy-cloud",
        name: "Calligraphy Cloud",
        description: "A dense, expressive marker and ink study.",
        category: "Sketch",
    },
    GalleryItem {
        id: "clean-stage",
        name: "Clean Stage",
        description: "An empty studio for a new sketch.",
        category: "Blank",
    },
];

pub fn item(id: &str) -> Option<GalleryItem> {
    GALLERY.iter().find(|g| g.id == id).copied()
}

pub fn build_document(id: &str) -> Option<Document> {
    let item = item(id)?;
    let library = BrushLibrary::default();
    let brush = |id: &str| library.get(id).cloned().unwrap_or_default();
    let mut doc = Document::new(item.name);
    doc.notes = item.description.into();
    doc.render.background.kind = BackgroundKind::Studio;
    doc.render.environment = Environment::default();
    doc.camera = Camera::new(
        Vec3::new(210.0, 155.0, 235.0),
        Vec3::new(0.0, 55.0, 0.0),
        38.0,
    );
    match id {
        "ribbon-bloom" => ribbon_bloom(&mut doc, &brush),
        "wire-flora" => wire_flora(&mut doc, &brush),
        "paper-lanterns" => paper_lanterns(&mut doc, &brush),
        "neon-knot" => neon_knot(&mut doc, &brush),
        "terrazzo-tile" => terrazzo_tile(&mut doc, &brush),
        "ink-orbit" => ink_orbit(&mut doc, &brush),
        "calligraphy-cloud" => calligraphy_cloud(&mut doc, &brush),
        "clean-stage" => clean_stage(&mut doc, &brush),
        _ => return None,
    }
    doc.animation
        .shots
        .push(crate::animation::CameraShot::from_camera(
            "Hero",
            &doc.camera,
            6.0,
        ));
    Some(doc)
}

fn add_curve(doc: &mut Document, mut curve: Curve) -> crate::CurveId {
    let id = doc.scene.new_curve_id();
    curve.id = id;
    curve.refresh_frames();
    doc.scene.add_curve(curve)
}

fn paint(mut brush: Brush, color: Vec4, size: f32) -> Brush {
    brush.color = color;
    brush.base_size = size;
    brush
}

fn ribbon_bloom(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    let palette = [
        Vec4::new(0.97, 0.43, 0.31, 1.0),
        Vec4::new(0.98, 0.68, 0.28, 1.0),
        Vec4::new(0.42, 0.74, 0.66, 1.0),
        Vec4::new(0.45, 0.60, 0.88, 1.0),
        Vec4::new(0.92, 0.55, 0.72, 1.0),
    ];
    for (i, color) in palette.iter().enumerate() {
        let base = (i as f32 - 2.0) * 22.0;
        let amp = 68.0 - i as f32 * 6.0;
        let n = 7;
        let points: Vec<Vec3> = (0..n)
            .map(|k| {
                let t = k as f32 / (n - 1) as f32;
                Vec3::new(
                    -210.0 + t * 420.0,
                    45.0 + base + (t * TAU * 1.35 + i as f32 * 0.7).sin() * amp,
                    -40.0 + (t * TAU * 0.7 + i as f32).cos() * 55.0,
                )
            })
            .collect();
        let mut c = Curve::from_positions(
            0,
            points,
            0.85,
            paint(brush("calligraphy-brush"), *color, 3.2 + i as f32 * 1.1),
        );
        c.kind = CurveKind::CatmullRom;
        c.name = format!("Ribbon {}", i + 1);
        c.tension = 0.35;
        add_curve(doc, c);
    }
    let mut g = Guide::new(
        0,
        "Bloom stage",
        crate::guide::GuideKind::Plane {
            width: 900.0,
            height: 620.0,
        },
    );
    g.transform = crate::Mat4::from_translation(Vec3::new(0.0, -20.0, 0.0));
    g.color = Vec4::new(0.48, 0.62, 0.74, 1.0);
    g.opacity = 0.18;
    g.rebuild();
    doc.scene.add_guide(g);
}

fn wire_flora(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    let stem = paint(brush("ink-fine"), Vec4::new(0.16, 0.78, 0.56, 1.0), 2.4);
    let stem_pts: Vec<Vec3> = (0..12)
        .map(|i| {
            let t = i as f32 / 11.0;
            Vec3::new(t.sin() * 28.0, -40.0 + t * 190.0, t.cos() * 18.0)
        })
        .collect();
    let mut s = Curve::from_positions(0, stem_pts, 0.8, stem.clone());
    s.kind = CurveKind::CatmullRom;
    s.name = "Stem".into();
    add_curve(doc, s);
    let petal_colors = [
        Vec4::new(0.98, 0.73, 0.32, 1.0),
        Vec4::new(0.96, 0.46, 0.38, 1.0),
        Vec4::new(0.58, 0.51, 0.90, 1.0),
        Vec4::new(0.42, 0.74, 0.78, 1.0),
    ];
    for (i, color) in petal_colors.iter().enumerate() {
        let height = 40.0 + i as f32 * 36.0;
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        let center = Vec3::new(side * (42.0 + i as f32 * 7.0), height, 10.0);
        let mut c = Curve::circle(
            0,
            center,
            20.0 + i as f32 * 4.0,
            Vec3::new(side, 0.35, 0.2),
            paint(brush("marker-medium"), *color, 2.2),
        );
        c.name = format!("Petal {}", i + 1);
        add_curve(doc, c);
        let leaf = Curve::ellipse(
            0,
            Vec3::new(-side * 24.0, height - 18.0, 0.0),
            16.0,
            5.0,
            side * 0.6,
            paint(brush("ink-fine"), Vec4::new(0.34, 0.72, 0.49, 1.0), 1.6),
        );
        add_curve(doc, leaf);
    }
}

fn paper_lanterns(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    let colors = [
        Vec4::new(0.98, 0.58, 0.34, 1.0),
        Vec4::new(0.96, 0.82, 0.47, 1.0),
        Vec4::new(0.89, 0.45, 0.56, 1.0),
    ];
    for (i, color) in colors.iter().enumerate() {
        let x = (i as f32 - 1.0) * 120.0;
        let y = 70.0 + (i as f32 * 1.7).sin() * 35.0;
        let mut c = Curve::circle(
            0,
            Vec3::new(x, y, 0.0),
            38.0 - i as f32 * 5.0,
            Vec3::Y,
            paint(brush("marker-medium"), *color, 1.8),
        );
        c.name = format!("Lantern {}", i + 1);
        c.closed = true;
        add_curve(doc, c);
        for k in 0..5 {
            let yy = y - 26.0 + k as f32 * 13.0;
            let rib = Curve::ellipse(
                0,
                Vec3::new(x, yy, 0.0),
                34.0 - (k as f32 - 2.0).abs() * 4.0,
                2.0,
                0.0,
                paint(brush("ink-fine"), *color, 0.9),
            );
            add_curve(doc, rib);
        }
        let string = Curve::line(
            0,
            Vec3::new(x, y + 38.0, 0.0),
            Vec3::new(x, y + 130.0, 0.0),
            paint(brush("ink-fine"), Vec4::new(0.62, 0.58, 0.50, 1.0), 0.5),
        );
        add_curve(doc, string);
    }
}

fn neon_knot(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    let colors = [
        Vec4::new(0.20, 0.95, 0.85, 1.0),
        Vec4::new(0.95, 0.31, 0.63, 1.0),
        Vec4::new(0.58, 0.54, 1.0, 1.0),
    ];
    for (k, color) in colors.iter().enumerate() {
        let mut b = paint(brush("neon-glow"), *color, 3.0);
        b.emission = 2.2;
        let n = 140;
        let points: Vec<Vec3> = (0..n)
            .map(|i| {
                let t = i as f32 / n as f32 * TAU;
                let r = 70.0 + 24.0 * (3.0 * t + k as f32 * 0.4).cos();
                Vec3::new(
                    r * t.cos(),
                    (3.0 * t).sin() * 42.0 + (k as f32 - 1.0) * 8.0,
                    r * t.sin(),
                )
            })
            .collect();
        let mut c = Curve::from_positions(0, points, 0.88, b);
        c.kind = CurveKind::CatmullRom;
        c.closed = true;
        c.name = format!("Neon strand {}", k + 1);
        add_curve(doc, c);
    }
    let mut g = Guide::torus(0, Vec3::ZERO, 92.0, 3.0);
    g.name = "Orbit".into();
    g.color = Vec4::new(0.32, 0.38, 0.52, 1.0);
    g.opacity = 0.22;
    doc.scene.add_guide(g);
}

fn terrazzo_tile(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    for i in 0..6 {
        let y = -110.0 + i as f32 * 45.0;
        let color = match i % 3 {
            0 => Vec4::new(0.92, 0.65, 0.47, 1.0),
            1 => Vec4::new(0.48, 0.72, 0.68, 1.0),
            _ => Vec4::new(0.82, 0.48, 0.57, 1.0),
        };
        let mut b = paint(brush("terrazzo-classic"), color, 28.0);
        // The terrazzo pattern fills cells with `color_b`; keep it in step with
        // the band colour so the loaded scene matches its gallery preview.
        b.pattern.color_b = color;
        b.pattern.spacing = 0.25;
        let mut c = Curve::line(0, Vec3::new(-180.0, y, 0.0), Vec3::new(180.0, y, 0.0), b);
        c.name = format!("Terrazzo band {}", i + 1);
        c.color = color;
        add_curve(doc, c);
    }
    doc.camera = Camera::new(Vec3::new(0.0, 0.0, 420.0), Vec3::ZERO, 38.0);
}

fn ink_orbit(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    for i in 0..5 {
        let radius = 48.0 + i as f32 * 22.0;
        let tilt = (i as f32 - 2.0) * 0.22;
        let rotation = crate::Quat::from_rotation_z(tilt);
        let mut c = Curve::circle(
            0,
            Vec3::ZERO,
            radius,
            Vec3::new(0.0, 1.0, 0.15).normalize(),
            paint(
                brush(if i % 2 == 0 {
                    "ink-fine"
                } else {
                    "calligraphy-brush"
                }),
                Vec4::new(0.94, 0.54, 0.36, 1.0),
                2.0,
            ),
        );
        for p in &mut c.points {
            p.position = rotation * p.position;
        }
        c.name = format!("Orbit {}", i + 1);
        add_curve(doc, c);
    }
    doc.scene.add_guide(Guide::sphere(0, Vec3::ZERO, 34.0));
}

fn calligraphy_cloud(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    let colors = [
        Vec4::new(0.93, 0.90, 0.82, 1.0),
        Vec4::new(0.88, 0.44, 0.33, 1.0),
        Vec4::new(0.45, 0.66, 0.82, 1.0),
    ];
    for i in 0..9 {
        let seed = i as f32 * 1.7;
        let pts: Vec<Vec3> = (0..6)
            .map(|k| {
                let t = k as f32 / 5.0;
                Vec3::new(
                    -200.0 + t * 400.0 + (seed + t * 4.0).cos() * 18.0,
                    20.0 + (seed * 0.7).sin() * 110.0 + (t * TAU + seed).sin() * 32.0,
                    (seed + t * 3.0).cos() * 30.0,
                )
            })
            .collect();
        let mut c = Curve::from_positions(
            0,
            pts,
            0.8,
            paint(
                brush(if i % 3 == 0 {
                    "calligraphy-brush"
                } else {
                    "ink-fine"
                }),
                colors[i % colors.len()],
                2.2 + (i % 4) as f32,
            ),
        );
        c.kind = CurveKind::CatmullRom;
        c.name = format!("Gesture {}", i + 1);
        add_curve(doc, c);
    }
}

fn clean_stage(doc: &mut Document, brush: &dyn Fn(&str) -> Brush) {
    doc.camera = Camera::default();
    let _ = brush;
}

/// Small CPU preview used by the gallery grid. It is intentionally lightweight
/// and deterministic; the real viewport renders with wgpu.
pub fn thumbnail(document: &Document, width: u32, height: u32) -> image::RgbaImage {
    let camera = framing_camera(document);
    render_preview(document, width, height, &camera)
}

/// Camera-aware CPU preview used by the mobile editor.  Unlike [`thumbnail`],
/// which always frames the whole scene, this keeps the document camera so that
/// orbiting and zooming are reflected immediately.
pub fn preview(document: &Document, width: u32, height: u32) -> image::RgbaImage {
    render_preview(document, width, height, &document.camera)
}

fn framing_camera(document: &Document) -> Camera {
    let (min, max) = document
        .scene
        .curves
        .iter()
        .filter_map(|c| {
            if c.points.is_empty() {
                None
            } else {
                Some(c.bounds())
            }
        })
        .fold(None, |acc: Option<(Vec3, Vec3)>, (a, b)| {
            Some(match acc {
                Some((mn, mx)) => (mn.min(a), mx.max(b)),
                None => (a, b),
            })
        })
        .unwrap_or((Vec3::splat(-80.0), Vec3::splat(80.0)));
    let centre = (min + max) * 0.5;
    let radius = (max - min).length().max(80.0);
    let mut camera = document.camera.clone();
    camera.target = centre;
    camera.position = centre + Vec3::new(0.8, 0.55, 1.0).normalize() * radius * 1.35;
    camera.fov_y_deg = 38.0;
    camera
}

fn render_preview(
    document: &Document,
    width: u32,
    height: u32,
    camera: &Camera,
) -> image::RgbaImage {
    let width = width.clamp(64, 1024);
    let height = height.clamp(48, 1024);
    let mut img = image::RgbaImage::from_pixel(width, height, image::Rgba([18, 20, 24, 255]));
    let viewport = Vec2::new(width as f32, height as f32);
    // Subtle vertical studio gradient.
    for y in 0..height {
        let t = y as f32 / height as f32;
        let c = [
            (22.0 + (1.0 - t) * 18.0) as u8,
            (24.0 + (1.0 - t) * 20.0) as u8,
            (29.0 + (1.0 - t) * 24.0) as u8,
            255,
        ];
        for x in 0..width {
            img.put_pixel(x, y, image::Rgba(c));
        }
    }
    for curve in &document.scene.curves {
        if !curve.visible {
            continue;
        }
        let samples = curve.sample((curve.brush.base_size * 0.7).max(1.5));
        let points: Vec<Vec2> = samples
            .samples
            .iter()
            .filter_map(|s| camera.project(s.position, viewport))
            .collect();
        let color = [
            (curve.color.x.clamp(0.0, 1.0) * 255.0) as u8,
            (curve.color.y.clamp(0.0, 1.0) * 255.0) as u8,
            (curve.color.z.clamp(0.0, 1.0) * 255.0) as u8,
            255,
        ];
        for w in points.windows(2) {
            draw_line(
                &mut img,
                w[0],
                w[1],
                color,
                if curve.color.x > 0.8 { 2 } else { 1 },
            );
        }
    }
    img
}

fn draw_line(img: &mut image::RgbaImage, a: Vec2, b: Vec2, color: [u8; 4], thickness: i32) {
    let (w, h) = img.dimensions();
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let steps = dx.abs().max(dy.abs()).max(1.0) as i32;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (a.x + dx * t).round() as i32;
        let y = (a.y + dy * t).round() as i32;
        for oy in -thickness..=thickness {
            for ox in -thickness..=thickness {
                let xx = x + ox;
                let yy = y + oy;
                if xx >= 0 && yy >= 0 && (xx as u32) < w && (yy as u32) < h {
                    img.put_pixel(xx as u32, yy as u32, image::Rgba(color));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gallery_item_builds_and_validates() {
        for item in GALLERY {
            let doc = build_document(item.id).unwrap_or_else(|| panic!("missing {}", item.id));
            assert_eq!(doc.name, item.name);
            let issues = doc.validate();
            assert!(issues.is_empty(), "{}: {issues:?}", item.id);
            if item.id != "clean-stage" {
                assert!(!doc.scene.curves.is_empty(), "{} is empty", item.id);
            }
        }
    }

    #[test]
    fn gallery_has_expected_count() {
        assert!(GALLERY.len() >= 8);
    }

    #[test]
    fn terrazzo_gallery_keeps_its_band_colours() {
        let doc = build_document("terrazzo-tile").unwrap();
        assert!(doc.scene.curves.len() >= 6);
        for curve in &doc.scene.curves {
            assert_eq!(curve.brush.pattern.color_b, curve.color);
        }
        let distinct: std::collections::BTreeSet<[u32; 4]> = doc
            .scene
            .curves
            .iter()
            .map(|curve| {
                [
                    curve.color.x.to_bits(),
                    curve.color.y.to_bits(),
                    curve.color.z.to_bits(),
                    curve.color.w.to_bits(),
                ]
            })
            .collect();
        assert!(distinct.len() >= 3);
    }

    #[test]
    fn gallery_thumbnail_has_content() {
        let doc = build_document("ribbon-bloom").unwrap();
        let img = thumbnail(&doc, 160, 120);
        let lit = img.pixels().filter(|p| p[0] > 60).count();
        assert!(lit > 40);
    }

    #[test]
    fn camera_preview_tracks_the_document_camera() {
        let mut doc = build_document("ribbon-bloom").unwrap();
        let before = preview(&doc, 160, 120);
        doc.camera.orbit_radians(1.4, 0.3);
        let after = preview(&doc, 160, 120);
        assert_ne!(before.as_raw(), after.as_raw());
        // The fitted thumbnail deliberately ignores the camera and should not
        // change when only the camera moves.
        let fitted_before = thumbnail(&doc, 160, 120);
        doc.camera.zoom(1.5);
        let fitted_after = thumbnail(&doc, 160, 120);
        assert_eq!(fitted_before.as_raw(), fitted_after.as_raw());
    }
}
