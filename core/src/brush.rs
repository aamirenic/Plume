//! Brush model, dynamics and the built-in brush library.

use crate::pattern::Pattern;
use crate::Vec2;
use crate::Vec4;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileShape {
    Round,
    Flat,
    Tapered,
    Custom,
}

impl ProfileShape {
    pub fn label(self) -> &'static str {
        match self {
            ProfileShape::Round => "Round",
            ProfileShape::Flat => "Flat",
            ProfileShape::Tapered => "Tapered",
            ProfileShape::Custom => "Custom",
        }
    }
}

/// Cross-section description. `custom` points are unit coordinates around the
/// origin; they are scaled by the brush radius during extrusion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushProfile {
    pub shape: ProfileShape,
    /// Preferred sides. Extrusion may lower this on straight segments but never
    /// below 3 (round) / 4 (custom).
    pub sides: u32,
    /// Flat profile width relative to its thickness.
    pub aspect: f32,
    /// Rotation of the profile around the tangent, in degrees.
    pub rotation_deg: f32,
    pub custom: Vec<Vec2>,
}

impl Default for BrushProfile {
    fn default() -> Self {
        Self {
            shape: ProfileShape::Round,
            sides: 16,
            aspect: 1.0,
            rotation_deg: 0.0,
            custom: Vec::new(),
        }
    }
}

impl BrushProfile {
    pub fn round(sides: u32) -> Self {
        Self {
            shape: ProfileShape::Round,
            sides: sides.clamp(3, 64),
            ..Self::default()
        }
    }

    pub fn flat(aspect: f32) -> Self {
        Self {
            shape: ProfileShape::Flat,
            sides: 4,
            aspect: aspect.max(0.05),
            ..Self::default()
        }
    }

    pub fn tapered() -> Self {
        Self {
            shape: ProfileShape::Tapered,
            sides: 3,
            ..Self::default()
        }
    }

    pub fn custom(points: Vec<Vec2>) -> Self {
        Self {
            shape: ProfileShape::Custom,
            sides: points.len().max(3) as u32,
            custom: points,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dynamics {
    /// 0 = pressure has no effect on size, 1 = full range.
    pub pressure_size: f32,
    /// 0 = pressure has no effect on opacity, 1 = transparent at 0 pressure.
    pub pressure_opacity: f32,
    /// Tilt tilts/thins the stroke.
    pub tilt_size: f32,
    pub tilt_opacity: f32,
    /// Fast strokes get thinner.
    pub velocity_size: f32,
    pub velocity_opacity: f32,
    /// Barrel roll rotates the cross-section.
    pub barrel_roll: f32,
    /// Piecewise linear map of raw pressure -> effective pressure.
    pub pressure_curve: Vec<Vec2>,
}

impl Default for Dynamics {
    fn default() -> Self {
        Self {
            pressure_size: 0.8,
            pressure_opacity: 0.25,
            tilt_size: 0.15,
            tilt_opacity: 0.0,
            velocity_size: 0.05,
            velocity_opacity: 0.0,
            barrel_roll: 1.0,
            pressure_curve: vec![
                Vec2::new(0.0, 0.25),
                Vec2::new(0.5, 0.7),
                Vec2::new(1.0, 1.0),
            ],
        }
    }
}

impl Dynamics {
    pub fn shape_pressure(&self, raw: f32) -> f32 {
        let p = raw.clamp(0.0, 1.0);
        if self.pressure_curve.is_empty() {
            return p;
        }
        let mut sorted: Vec<Vec2> = self.pressure_curve.clone();
        sorted.sort_by(|a, b| a.x.total_cmp(&b.x));
        if p <= sorted[0].x {
            return sorted[0].y.clamp(0.0, 1.0);
        }
        for w in sorted.windows(2) {
            if p <= w[1].x {
                let dx = (w[1].x - w[0].x).max(1.0e-6);
                let t = (p - w[0].x) / dx;
                return (w[0].y + (w[1].y - w[0].y) * t).clamp(0.0, 1.0);
            }
        }
        sorted.last().map(|v| v.y).unwrap_or(p).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Taper {
    pub enabled: bool,
    /// Fraction of the stroke (0..1) used for the entry taper.
    pub start: f32,
    /// Fraction of the stroke used for the exit taper.
    pub end: f32,
    /// Additional pressure-driven taper.
    pub pressure: f32,
    /// Extra global radius multiplier at the very tip.
    pub tip: f32,
}

impl Default for Taper {
    fn default() -> Self {
        Self {
            enabled: true,
            start: 0.06,
            end: 0.1,
            pressure: 0.3,
            tip: 0.08,
        }
    }
}

impl Taper {
    pub fn factor(&self, u: f32) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        let u = u.clamp(0.0, 1.0);
        let a = if self.start > 1.0e-4 {
            let t = (u / self.start).clamp(0.0, 1.0);
            self.tip + (1.0 - self.tip) * (t * t * (3.0 - 2.0 * t))
        } else {
            1.0
        };
        let b = if self.end > 1.0e-4 {
            let t = ((1.0 - u) / self.end).clamp(0.0, 1.0);
            self.tip + (1.0 - self.tip) * (t * t * (3.0 - 2.0 * t))
        } else {
            1.0
        };
        (a * b).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stabilization {
    pub strength: f32,
    pub stable_stroke: bool,
    /// Artificial lag in seconds for stable stroke.
    pub delay: f32,
    pub mouse_pressure: f32,
}

impl Default for Stabilization {
    fn default() -> Self {
        Self {
            strength: 0.18,
            stable_stroke: false,
            delay: 0.03,
            mouse_pressure: 0.85,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrushCategory {
    Sketch,
    Marker,
    Calligraphy,
    Wide,
    Dots,
    Hatching,
    Halftone,
    Terrazzo,
    Stipple,
    Smooth,
    Rough,
    Taper,
    Ink,
    Experimental,
}

impl BrushCategory {
    pub fn label(self) -> &'static str {
        match self {
            BrushCategory::Sketch => "Sketch",
            BrushCategory::Marker => "Marker",
            BrushCategory::Calligraphy => "Calligraphy",
            BrushCategory::Wide => "Wide",
            BrushCategory::Dots => "Dots",
            BrushCategory::Hatching => "Hatching",
            BrushCategory::Halftone => "Halftone",
            BrushCategory::Terrazzo => "Terrazzo",
            BrushCategory::Stipple => "Stipple",
            BrushCategory::Smooth => "Smooth",
            BrushCategory::Rough => "Rough",
            BrushCategory::Taper => "Taper",
            BrushCategory::Ink => "Ink",
            BrushCategory::Experimental => "Experimental",
        }
    }
}

/// Fully resolved parameters for one stroke.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Brush {
    pub id: String,
    pub name: String,
    pub category: BrushCategory,
    pub profile: BrushProfile,
    /// Diameter in world units (millimetres).
    pub base_size: f32,
    pub min_size: f32,
    pub max_size: f32,
    pub color: Vec4,
    pub opacity: f32,
    pub pattern: Pattern,
    pub dynamics: Dynamics,
    pub stabilization: Stabilization,
    pub taper: Taper,
    pub builtin: bool,
    /// Emissive strength for neon-like brushes.
    pub emission: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            id: "sketch-medium".into(),
            name: "Sketch Medium".into(),
            category: BrushCategory::Sketch,
            profile: BrushProfile::round(16),
            base_size: 4.0,
            min_size: 0.35,
            max_size: 40.0,
            color: Vec4::new(0.88, 0.87, 0.82, 1.0),
            opacity: 0.96,
            pattern: Pattern::solid(),
            dynamics: Dynamics::default(),
            stabilization: Stabilization::default(),
            taper: Taper::default(),
            builtin: true,
            emission: 0.0,
        }
    }
}

/// Inputs from the stylus/mouse at a single dab.
#[derive(Clone, Copy, Debug)]
pub struct BrushInput {
    pub pressure: f32,
    pub tilt: f32,
    pub velocity: f32,
    pub roll_rad: f32,
    /// 0..1 position along the stroke.
    pub u: f32,
}

impl Default for BrushInput {
    fn default() -> Self {
        Self {
            pressure: 0.85,
            tilt: 0.0,
            velocity: 0.0,
            roll_rad: 0.0,
            u: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BrushDab {
    /// Radius in world units.
    pub radius: f32,
    pub opacity: f32,
    pub color: Vec4,
    pub roll_rad: f32,
    pub emission: f32,
}

impl Brush {
    pub fn with_size(mut self, size: f32) -> Self {
        self.base_size = size.clamp(self.min_size, self.max_size);
        self
    }

    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_pattern(mut self, pattern: Pattern) -> Self {
        self.pattern = pattern;
        self
    }

    pub fn resolve(&self, input: BrushInput) -> BrushDab {
        let shaped = self.dynamics.shape_pressure(input.pressure);
        let tilt = input.tilt.clamp(0.0, 1.0);
        let velocity = input.velocity.clamp(0.0, 1.0);
        let size_factor = (1.0 - self.dynamics.pressure_size * (1.0 - shaped))
            * (1.0 - self.dynamics.tilt_size * tilt * 0.35)
            * (1.0 - self.dynamics.velocity_size * velocity);
        let taper = self.taper.factor(input.u);
        let pressure_taper = if self.taper.enabled {
            1.0 - self.taper.pressure * (1.0 - shaped) * 0.5
        } else {
            1.0
        };
        let diameter = (self.base_size * size_factor * taper * pressure_taper)
            .clamp(self.min_size, self.max_size);
        let opacity = (self.opacity
            * (1.0 - self.dynamics.pressure_opacity * (1.0 - shaped))
            * (1.0 - self.dynamics.tilt_opacity * tilt)
            * (1.0 - self.dynamics.velocity_opacity * velocity))
            .clamp(0.0, 1.0);
        BrushDab {
            radius: (diameter * 0.5).max(0.02),
            opacity,
            color: self.color,
            roll_rad: input.roll_rad * self.dynamics.barrel_roll,
            emission: self.emission,
        }
    }

    /// Radius at a normalised stroke position with a representative pressure.
    pub fn preview_radius(&self, u: f32, pressure: f32) -> f32 {
        self.resolve(BrushInput {
            pressure,
            u,
            ..BrushInput::default()
        })
        .radius
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if !(self.base_size.is_finite() && self.base_size > 0.0) {
            return Err("base size must be finite and > 0".into());
        }
        if self.min_size > self.max_size {
            return Err("min size must not exceed max size".into());
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err("opacity must be in 0..1".into());
        }
        if self.profile.sides < 3 {
            return Err("profile needs at least 3 sides".into());
        }
        Ok(())
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "format": "plume-brush",
            "version": 1,
            "brush": self,
        })
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.to_json_value()).unwrap_or_default()
    }

    pub fn from_json_str(s: &str) -> crate::Result<Self> {
        let value: serde_json::Value = serde_json::from_str(s)?;
        let brush_value = value.get("brush").cloned().unwrap_or(value);
        Self::from_value(&brush_value)
    }

    fn from_value(value: &serde_json::Value) -> crate::Result<Self> {
        if let Ok(brush) = serde_json::from_value::<Brush>(value.clone()) {
            return Ok(brush);
        }
        // Best-effort mapping for .feather-brush and older/unknown brush files.
        let mut brush = Brush::default();
        let get_str = |keys: &[&str]| -> Option<String> {
            keys.iter()
                .find_map(|k| value.get(*k).and_then(|v| v.as_str()).map(str::to_owned))
        };
        let get_num = |keys: &[&str]| -> Option<f32> {
            keys.iter().find_map(|k| {
                value.get(*k).and_then(|v| {
                    v.as_f64()
                        .map(|f| f as f32)
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
            })
        };
        if let Some(name) = get_str(&["name", "title", "id"]) {
            brush.name = name;
            if brush.id.is_empty() {
                brush.id = slug(&brush.name);
            }
        }
        if let Some(size) = get_num(&["base_size", "size", "diameter", "brushSize"]) {
            brush.base_size = size.max(0.01);
            brush.max_size = brush.max_size.max(brush.base_size * 4.0);
        }
        if let Some(opacity) = get_num(&["opacity", "alpha"]) {
            brush.opacity = if opacity > 1.0 {
                opacity / 100.0
            } else {
                opacity
            }
            .clamp(0.0, 1.0);
        }
        if let Some(p) = get_num(&["pressure_size", "pressureSize", "pressure"]) {
            brush.dynamics.pressure_size = p.clamp(0.0, 1.0);
        }
        if let Some(color) = value.get("color").and_then(|c| c.as_array()) {
            let vals: Vec<f32> = color
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            if vals.len() >= 3 {
                brush.color = Vec4::new(
                    vals[0],
                    vals[1],
                    vals[2],
                    vals.get(3).copied().unwrap_or(1.0),
                );
                if vals.iter().take(3).any(|v| *v > 1.0) {
                    brush.color /= 255.0;
                }
            }
        }
        if let Some(kind) = get_str(&["pattern", "patternType"]) {
            let lower = kind.to_lowercase();
            if lower.contains("half") {
                brush.pattern = Pattern::halftone(0.25, 0.18);
            } else if lower.contains("cross") {
                brush.pattern = Pattern::cross_hatch(0.25);
            } else if lower.contains("hatch") {
                brush.pattern = Pattern::hatch(45.0, 0.25);
            } else if lower.contains("stip") {
                brush.pattern = Pattern::stipple(0.5);
            } else if lower.contains("terra") || lower.contains("voronoi") {
                brush.pattern = Pattern::terrazzo(0.25);
            }
        }
        brush.id = if brush.id.trim().is_empty() {
            slug(&brush.name)
        } else {
            brush.id.clone()
        };
        brush.builtin = false;
        Ok(brush)
    }
}

pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// The user's brush library: built-ins plus imported brushes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrushLibrary {
    pub brushes: Vec<Brush>,
    #[serde(default)]
    pub favorites: Vec<String>,
}

impl Default for BrushLibrary {
    fn default() -> Self {
        Self {
            brushes: builtin_brushes(),
            favorites: vec!["ink-fine".into(), "sketch-medium".into()],
        }
    }
}

impl BrushLibrary {
    pub fn search(&self, query: &str) -> Vec<&Brush> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.brushes.iter().collect();
        }
        self.brushes
            .iter()
            .filter(|b| {
                b.name.to_lowercase().contains(&q)
                    || b.category.label().to_lowercase().contains(&q)
                    || b.id.contains(&q)
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&Brush> {
        self.brushes.iter().find(|b| b.id == id)
    }

    pub fn add(&mut self, brush: Brush) -> String {
        let mut id = if brush.id.is_empty() {
            slug(&brush.name)
        } else {
            brush.id.clone()
        };
        let base = id.clone();
        let mut n = 2;
        while self.brushes.iter().any(|b| b.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        let mut brush = brush;
        brush.id = id.clone();
        brush.builtin = false;
        self.brushes.push(brush);
        id
    }

    pub fn duplicate(&mut self, id: &str) -> Option<String> {
        let mut brush = self.get(id)?.clone();
        brush.name = format!("{} copy", brush.name);
        brush.id.clear();
        Some(self.add(brush))
    }

    pub fn remove(&mut self, id: &str) -> bool {
        if let Some(pos) = self.brushes.iter().position(|b| b.id == id) {
            if self.brushes[pos].builtin {
                return false;
            }
            self.brushes.remove(pos);
            self.favorites.retain(|f| f != id);
            true
        } else {
            false
        }
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.brushes.len() || to >= self.brushes.len() || from == to {
            return;
        }
        let brush = self.brushes.remove(from);
        self.brushes.insert(to, brush);
    }

    pub fn import_str(&mut self, s: &str) -> crate::Result<String> {
        let brush = Brush::from_json_str(s)?;
        Ok(self.add(brush))
    }
}

fn brush(
    id: &str,
    name: &str,
    category: BrushCategory,
    profile: BrushProfile,
    size: f32,
    color: Vec4,
    pattern: Pattern,
) -> Brush {
    Brush {
        id: id.into(),
        name: name.into(),
        category,
        profile,
        base_size: size,
        color,
        pattern,
        ..Brush::default()
    }
}

/// 33 built-in presets. They are generated, never loaded from external art.
pub fn builtin_brushes() -> Vec<Brush> {
    let paper = Vec4::new(0.90, 0.89, 0.84, 1.0);
    let graphite = Vec4::new(0.72, 0.74, 0.78, 1.0);
    let ink = Vec4::new(0.06, 0.07, 0.09, 1.0);
    let warm = Vec4::new(0.97, 0.55, 0.30, 1.0);
    let mint = Vec4::new(0.38, 0.86, 0.72, 1.0);
    let iris = Vec4::new(0.56, 0.47, 0.98, 1.0);
    let lemon = Vec4::new(0.96, 0.79, 0.28, 1.0);

    let mut v = Vec::new();
    for (id, name, size) in [
        ("sketch-fine", "Sketch Fine", 1.6),
        ("sketch-medium", "Sketch Medium", 4.0),
        ("sketch-bold", "Sketch Bold", 9.0),
    ] {
        v.push(brush(
            id,
            name,
            BrushCategory::Sketch,
            BrushProfile::round(16),
            size,
            paper,
            Pattern::solid(),
        ));
    }
    for (id, name, size, aspect) in [
        ("marker-fine", "Marker Fine", 5.0, 0.35),
        ("marker-medium", "Marker Medium", 11.0, 0.28),
        ("marker-bold", "Marker Bold", 22.0, 0.22),
    ] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Marker,
            BrushProfile::flat(aspect),
            size,
            warm,
            Pattern::solid(),
        );
        b.opacity = 0.9;
        b.taper.enabled = false;
        b.dynamics.pressure_size = 0.15;
        v.push(b);
    }
    for (id, name, aspect, size) in [
        ("calligraphy-pointed", "Calligraphy Pointed", 0.12, 7.0),
        ("calligraphy-oblique", "Calligraphy Oblique", 0.4, 9.0),
        ("calligraphy-brush", "Calligraphy Brush", 0.22, 13.0),
    ] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Calligraphy,
            BrushProfile::flat(aspect),
            size,
            ink,
            Pattern::solid(),
        );
        b.profile.rotation_deg = 55.0;
        b.dynamics.pressure_size = 0.95;
        b.taper.enabled = true;
        b.stabilization.strength = 0.3;
        v.push(b);
    }
    for (id, name, size) in [
        ("wide-large", "Wide Large", 34.0),
        ("wide-medium", "Wide Medium", 20.0),
    ] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Wide,
            BrushProfile::flat(0.5),
            size,
            Vec4::new(0.35, 0.62, 0.96, 1.0),
            Pattern::solid(),
        );
        b.dynamics.pressure_size = 0.0;
        b.opacity = 0.75;
        b.taper.enabled = false;
        v.push(b);
    }
    for (id, name, size) in [
        ("dots-small", "Dots Small", 6.0),
        ("dots-large", "Dots Large", 16.0),
    ] {
        v.push(brush(
            id,
            name,
            BrushCategory::Dots,
            BrushProfile::round(12),
            size,
            lemon,
            Pattern::stipple(0.5),
        ));
    }
    for (id, name, pattern) in [
        (
            "hatching-diagonal",
            "Hatching Diagonal",
            Pattern::hatch(45.0, 0.35),
        ),
        (
            "hatching-cross",
            "Hatching Cross",
            Pattern::cross_hatch(0.3),
        ),
    ] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Hatching,
            BrushProfile::flat(0.6),
            12.0,
            mint,
            pattern,
        );
        b.opacity = 0.95;
        b.dynamics.pressure_size = 0.3;
        v.push(b);
    }
    for (id, name, size, spacing) in [
        ("halftone-dot-grid", "Halftone Dot Grid", 14.0, 0.3),
        ("halftone-fine", "Halftone Fine", 8.0, 0.18),
        ("halftone-bold", "Halftone Bold", 22.0, 0.42),
    ] {
        v.push(brush(
            id,
            name,
            BrushCategory::Halftone,
            BrushProfile::round(20),
            size,
            iris,
            Pattern::halftone(spacing, 0.19),
        ));
    }
    for (id, name, size, spacing) in [
        ("terrazzo-classic", "Terrazzo Classic", 22.0, 0.32),
        ("terrazzo-fine", "Terrazzo Fine", 12.0, 0.2),
    ] {
        v.push(brush(
            id,
            name,
            BrushCategory::Terrazzo,
            BrushProfile::round(24),
            size,
            Vec4::new(0.92, 0.74, 0.56, 1.0),
            Pattern::terrazzo(spacing),
        ));
    }
    for (id, name, density) in [
        ("stipple-fine", "Stipple Fine", 0.32),
        ("stipple-dense", "Stipple Dense", 0.72),
    ] {
        v.push(brush(
            id,
            name,
            BrushCategory::Stipple,
            BrushProfile::round(16),
            11.0,
            Vec4::new(0.95, 0.42, 0.57, 1.0),
            Pattern::stipple(density),
        ));
    }
    for (id, name, size) in [
        ("smooth-clean", "Smooth Clean", 3.0),
        ("smooth-wide", "Smooth Wide", 8.0),
    ] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Smooth,
            BrushProfile::round(20),
            size,
            paper,
            Pattern::solid(),
        );
        b.stabilization.strength = 0.85;
        b.stabilization.stable_stroke = true;
        b.taper.enabled = false;
        v.push(b);
    }
    for (id, name, jitter, size) in [
        ("rough-graphite", "Rough Graphite", 0.6, 5.0),
        ("rough-charcoal", "Rough Charcoal", 0.9, 12.0),
    ] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Rough,
            BrushProfile::round(10),
            size,
            graphite,
            Pattern::hatch(18.0, 0.13),
        );
        b.dynamics.pressure_size = jitter;
        b.stabilization.strength = 0.02;
        b.taper.enabled = false;
        v.push(b);
    }
    for (id, name, size) in [
        ("taper-fine", "Taper Fine", 5.0),
        ("taper-bold", "Taper Bold", 14.0),
    ] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Taper,
            BrushProfile::round(18),
            size,
            Vec4::new(0.98, 0.72, 0.36, 1.0),
            Pattern::solid(),
        );
        b.taper.enabled = true;
        b.taper.start = 0.16;
        b.taper.end = 0.22;
        b.dynamics.pressure_size = 0.9;
        v.push(b);
    }
    for (id, name, size) in [("ink-fine", "Ink Fine", 2.2), ("ink-bold", "Ink Bold", 7.5)] {
        let mut b = brush(
            id,
            name,
            BrushCategory::Ink,
            BrushProfile::round(16),
            size,
            Vec4::new(0.08, 0.1, 0.14, 1.0),
            Pattern::solid(),
        );
        b.dynamics.pressure_size = 0.6;
        b.stabilization.strength = 0.4;
        v.push(b);
    }
    let mut neon = brush(
        "neon-glow",
        "Neon Glow",
        BrushCategory::Experimental,
        BrushProfile::round(16),
        3.5,
        Vec4::new(0.42, 0.95, 0.87, 1.0),
        Pattern::solid(),
    );
    neon.emission = 1.6;
    neon.opacity = 1.0;
    v.push(neon);

    let mut ribbon = brush(
        "ribbon-silk",
        "Ribbon Silk",
        BrushCategory::Experimental,
        BrushProfile::flat(0.08),
        15.0,
        Vec4::new(0.72, 0.58, 1.0, 1.0),
        Pattern::solid(),
    );
    ribbon.dynamics.barrel_roll = 1.0;
    ribbon.taper.enabled = true;
    v.push(ribbon);

    let mut sprayed = brush(
        "spray-soft",
        "Spray Soft",
        BrushCategory::Experimental,
        BrushProfile::round(12),
        17.0,
        Vec4::new(0.93, 0.48, 0.79, 1.0),
        Pattern::stipple(0.85),
    );
    sprayed.opacity = 0.45;
    sprayed.dynamics.pressure_opacity = 0.8;
    v.push(sprayed);

    // 33 total; every id is unique.
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::PatternKind;

    #[test]
    fn preset_count_matches_documentation() {
        let brushes = builtin_brushes();
        assert_eq!(brushes.len(), 33, "update the documented count");
    }

    #[test]
    fn preset_ids_unique() {
        let brushes = builtin_brushes();
        let mut ids = std::collections::HashSet::new();
        for b in &brushes {
            assert!(ids.insert(b.id.clone()), "duplicate id {}", b.id);
            b.validate().unwrap();
            assert!(b.base_size >= b.min_size && b.base_size <= b.max_size);
        }
    }

    #[test]
    fn pressure_changes_size() {
        let b = Brush::default();
        let low = b.resolve(BrushInput {
            pressure: 0.05,
            u: 0.5,
            ..BrushInput::default()
        });
        let high = b.resolve(BrushInput {
            pressure: 1.0,
            u: 0.5,
            ..BrushInput::default()
        });
        assert!(high.radius > low.radius * 2.0);
        assert!(high.opacity >= low.opacity);
    }

    #[test]
    fn taper_ends_are_thinner() {
        let mut b = Brush::default();
        b.taper.enabled = true;
        b.taper.start = 0.2;
        b.taper.end = 0.2;
        let mid = b.preview_radius(0.5, 0.8);
        let start = b.preview_radius(0.0, 0.8);
        assert!(start < mid * 0.4);
    }

    #[test]
    fn brush_json_roundtrip() {
        let mut b = builtin_brushes().remove(4);
        b.id = "custom-test".into();
        b.builtin = false;
        let json = b.to_json_pretty();
        let b2 = Brush::from_json_str(&json).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn feather_brush_best_effort() {
        let json = r#"{"name":"Wet Ink","size":12.5,"opacity":0.6,"color":[0.2,0.4,0.9,1.0],"pattern":"halftone"}"#;
        let b = Brush::from_json_str(json).unwrap();
        assert_eq!(b.name, "Wet Ink");
        assert!((b.base_size - 12.5).abs() < 1e-6);
        assert_eq!(b.pattern.kind, PatternKind::Halftone);
        assert!((b.opacity - 0.6).abs() < 1e-6);
    }

    #[test]
    fn library_search_and_duplicate() {
        let mut lib = BrushLibrary::default();
        assert!(lib.search("halftone").len() >= 3);
        let id = lib.duplicate("ink-fine").unwrap();
        assert!(lib.get(&id).is_some());
        assert!(lib.remove(&id));
        assert!(!lib.remove("ink-fine"));
    }

    #[test]
    fn all_presets_have_geometry_parameters() {
        for b in builtin_brushes() {
            assert!(b.profile.sides >= 3);
            assert!(b.base_size > 0.0);
            let dab = b.resolve(BrushInput {
                pressure: 0.7,
                u: 0.5,
                ..BrushInput::default()
            });
            assert!(dab.radius > 0.0);
            assert!(dab.opacity > 0.0);
        }
    }
}
