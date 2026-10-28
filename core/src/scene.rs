//! Document model: curves, guides, groups, selection, mirroring and history.
//!
//! The scene deliberately keeps GPU and BVH caches out of serialisation.  A
//! `.plume` document contains only authored data; meshes are rebuilt from the
//! curves when a renderer, exporter or gallery needs them.

use crate::animation::Animation;
use crate::brush::Brush;
use crate::curve::{mirror_curve, Curve};
use crate::extrusion::{extrude_curve, ExtrusionOptions};
use crate::guide::{Guide, GuideKind};
use crate::mesh::Mesh;
use crate::render::RenderSettings;
use crate::viewport::Camera;
use crate::{CurveId, GroupId, GuideId, Mat4, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PLUME_SCHEMA: &str = "plume.document";
pub const PLUME_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeshQuality {
    Draft,
    Normal,
    High,
    Export,
}

impl MeshQuality {
    pub fn spacing(self) -> f32 {
        match self {
            MeshQuality::Draft => 4.0,
            MeshQuality::Normal => 2.0,
            MeshQuality::High => 1.2,
            MeshQuality::Export => 0.8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MeshQuality::Draft => "Draft",
            MeshQuality::Normal => "Normal",
            MeshQuality::High => "High",
            MeshQuality::Export => "Export",
        }
    }
}

/// Build the render/export mesh for one curve.
pub fn curve_mesh(curve: &Curve, quality: MeshQuality) -> Mesh {
    if curve.points.is_empty() {
        return Mesh::new();
    }
    let ribbon = matches!(curve.brush.profile.shape, crate::brush::ProfileShape::Flat)
        && curve.brush.profile.aspect <= 0.18;
    let options = ExtrusionOptions {
        spacing: quality.spacing().min(curve.brush.base_size.max(0.6) * 0.75),
        adaptive_sides: !matches!(quality, MeshQuality::Export | MeshQuality::High),
        min_sides: 8,
        max_sides: if matches!(quality, MeshQuality::Export) {
            96
        } else {
            64
        },
        caps: !curve.closed,
        smooth_normals: true,
        ribbon,
        two_sided: ribbon,
        weld: matches!(quality, MeshQuality::Export),
        ..ExtrusionOptions::default()
    };
    let mut mesh = extrude_curve(curve, &options);
    if mesh.is_empty() {
        // Ensure a single-point stroke still has a tiny cap so it is visible
        // and selectable in exports.
        if let Some(p) = curve.points.first() {
            let r = (curve.brush.base_size * 0.5).max(0.05);
            let a = mesh.add_vertex(p.position + Vec3::Y * r, Vec3::Y, crate::Vec2::splat(0.5));
            let b = mesh.add_vertex(p.position - Vec3::X * r, Vec3::Y, crate::Vec2::ZERO);
            let c = mesh.add_vertex(p.position + Vec3::X * r, Vec3::Y, crate::Vec2::X);
            mesh.push_triangle(a, b, c);
        }
    }
    mesh
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub parent: Option<GroupId>,
    pub visible: bool,
    pub locked: bool,
    pub color: Vec4,
    pub expanded: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub curves: BTreeSet<CurveId>,
    pub guides: BTreeSet<GuideId>,
    pub groups: BTreeSet<GroupId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.curves.is_empty() && self.guides.is_empty() && self.groups.is_empty()
    }

    pub fn clear(&mut self) {
        self.curves.clear();
        self.guides.clear();
        self.groups.clear();
    }

    pub fn only_curve(&mut self, id: CurveId) {
        self.clear();
        self.curves.insert(id);
    }

    pub fn only_guide(&mut self, id: GuideId) {
        self.clear();
        self.guides.insert(id);
    }

    pub fn toggle_curve(&mut self, id: CurveId) {
        if !self.curves.remove(&id) {
            self.curves.insert(id);
        }
    }

    pub fn toggle_guide(&mut self, id: GuideId) {
        if !self.guides.remove(&id) {
            self.guides.insert(id);
        }
    }

    pub fn count(&self) -> usize {
        self.curves.len() + self.guides.len() + self.groups.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirrorAxis {
    X,
    Y,
    Z,
}

impl MirrorAxis {
    pub fn label(self) -> &'static str {
        match self {
            MirrorAxis::X => "X",
            MirrorAxis::Y => "Y",
            MirrorAxis::Z => "Z",
        }
    }

    pub fn normal(self) -> Vec3 {
        match self {
            MirrorAxis::X => Vec3::X,
            MirrorAxis::Y => Vec3::Y,
            MirrorAxis::Z => Vec3::Z,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MirrorSettings {
    pub enabled: bool,
    pub axis: MirrorAxis,
    /// Plane position along the selected axis.
    pub offset: f32,
    /// Mirror live strokes as they are drawn.
    pub live: bool,
    pub mirror_guides: bool,
    /// Curves whose mirrored copy has been baked are kept out of live mode by
    /// this tag; exporting never duplicates tagged curves again.
    pub merge_threshold: f32,
}

impl Default for MirrorSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            axis: MirrorAxis::X,
            offset: 0.0,
            live: false,
            mirror_guides: false,
            merge_threshold: 0.15,
        }
    }
}

impl MirrorSettings {
    pub fn plane_point(&self) -> Vec3 {
        self.axis.normal() * self.offset
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub curves: Vec<Curve>,
    pub guides: Vec<Guide>,
    pub groups: Vec<Group>,
    pub selection: Selection,
    pub mirror: MirrorSettings,
    pub active_guide: Option<GuideId>,
    pub next_curve_id: u64,
    pub next_guide_id: u64,
    pub next_group_id: u64,
    #[serde(skip)]
    pub revision: u64,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            curves: Vec::new(),
            guides: Vec::new(),
            groups: Vec::new(),
            selection: Selection::default(),
            mirror: MirrorSettings::default(),
            active_guide: None,
            next_curve_id: 1,
            next_guide_id: 1,
            next_group_id: 1,
            revision: 1,
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1).max(1);
    }

    pub fn new_curve_id(&mut self) -> CurveId {
        let id = self.next_curve_id.max(1);
        self.next_curve_id = id + 1;
        id
    }

    pub fn new_guide_id(&mut self) -> GuideId {
        let id = self.next_guide_id.max(1);
        self.next_guide_id = id + 1;
        id
    }

    pub fn new_group_id(&mut self) -> GroupId {
        let id = self.next_group_id.max(1);
        self.next_group_id = id + 1;
        id
    }

    pub fn add_curve(&mut self, mut curve: Curve) -> CurveId {
        if curve.id == 0 || self.curves.iter().any(|c| c.id == curve.id) {
            curve.id = self.new_curve_id();
        } else {
            self.next_curve_id = self.next_curve_id.max(curve.id + 1);
        }
        let id = curve.id;
        self.curves.push(curve);
        self.touch();
        id
    }

    pub fn add_guide(&mut self, mut guide: Guide) -> GuideId {
        if guide.id == 0 || self.guides.iter().any(|g| g.id == guide.id) {
            guide.id = self.new_guide_id();
        } else {
            self.next_guide_id = self.next_guide_id.max(guide.id + 1);
        }
        let id = guide.id;
        self.guides.push(guide);
        // Keep the convenience field and the per-guide flags in sync.
        self.set_active_guide(Some(id));
        id
    }

    pub fn remove_curve(&mut self, id: CurveId) -> Option<Curve> {
        let index = self.curves.iter().position(|c| c.id == id)?;
        let curve = self.curves.remove(index);
        self.selection.curves.remove(&id);
        self.touch();
        Some(curve)
    }

    pub fn remove_guide(&mut self, id: GuideId) -> Option<Guide> {
        let index = self.guides.iter().position(|g| g.id == id)?;
        let guide = self.guides.remove(index);
        self.selection.guides.remove(&id);
        if self.active_guide == Some(id) {
            self.active_guide = None;
        }
        self.touch();
        Some(guide)
    }

    pub fn curve(&self, id: CurveId) -> Option<&Curve> {
        self.curves.iter().find(|c| c.id == id)
    }

    pub fn curve_mut(&mut self, id: CurveId) -> Option<&mut Curve> {
        self.touch();
        self.curves.iter_mut().find(|c| c.id == id)
    }

    pub fn guide(&self, id: GuideId) -> Option<&Guide> {
        self.guides.iter().find(|g| g.id == id)
    }

    pub fn guide_mut(&mut self, id: GuideId) -> Option<&mut Guide> {
        self.touch();
        self.guides.iter_mut().find(|g| g.id == id)
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.touch();
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub fn active_guide_mut(&mut self) -> Option<&mut Guide> {
        let id = self.active_guide?;
        self.guides.iter_mut().find(|g| g.id == id && g.visible)
    }

    pub fn set_active_guide(&mut self, id: Option<GuideId>) {
        self.active_guide = id;
        for g in &mut self.guides {
            g.active = Some(g.id) == id;
        }
        self.touch();
    }

    /// Delete every selected curve, guide and group. Group deletion never
    /// deletes art accidentally; children are moved to the parent group.
    pub fn delete_selection(&mut self) -> usize {
        let curve_ids: Vec<_> = self.selection.curves.iter().copied().collect();
        let guide_ids: Vec<_> = self.selection.guides.iter().copied().collect();
        let group_ids: Vec<_> = self.selection.groups.iter().copied().collect();
        let mut removed = 0;
        for id in curve_ids {
            if self.remove_curve(id).is_some() {
                removed += 1;
            }
        }
        for id in guide_ids {
            if self.remove_guide(id).is_some() {
                removed += 1;
            }
        }
        for id in group_ids {
            if let Some(index) = self.groups.iter().position(|g| g.id == id) {
                let parent = self.groups[index].parent;
                for g in &mut self.groups {
                    if g.parent == Some(id) {
                        g.parent = parent;
                    }
                }
                for c in &mut self.curves {
                    if c.group == Some(id) {
                        c.group = parent;
                    }
                }
                self.groups.remove(index);
                removed += 1;
            }
        }
        self.selection.clear();
        self.touch();
        removed
    }

    pub fn duplicate_curve(&mut self, id: CurveId) -> Option<CurveId> {
        let original = self.curve(id)?.clone();
        let new_id = self.new_curve_id();
        let mut copy = original;
        copy.id = new_id;
        copy.name = format!("{} copy", copy.name);
        copy.tag = None;
        copy.refresh_frames();
        self.curves.push(copy);
        self.selection.only_curve(new_id);
        self.touch();
        Some(new_id)
    }

    pub fn duplicate_selection(&mut self) -> Vec<CurveId> {
        let mut created = Vec::new();
        for id in self.selection.curves.clone() {
            if let Some(new_id) = self.duplicate_curve(id) {
                created.push(new_id);
            }
        }
        self.touch();
        created
    }

    pub fn group_selection(&mut self, name: impl Into<String>) -> Option<GroupId> {
        if self.selection.curves.is_empty() {
            return None;
        }
        let id = self.new_group_id();
        self.groups.push(Group {
            id,
            name: name.into(),
            parent: None,
            visible: true,
            locked: false,
            color: Vec4::new(0.58, 0.76, 0.70, 1.0),
            expanded: true,
        });
        for c in &mut self.curves {
            if self.selection.curves.contains(&c.id) {
                c.group = Some(id);
            }
        }
        self.selection.groups.insert(id);
        self.touch();
        Some(id)
    }

    pub fn ungroup_selection(&mut self) -> usize {
        let mut count = 0;
        for group_id in self.selection.groups.clone() {
            let parent = self.group(group_id).and_then(|g| g.parent);
            for c in &mut self.curves {
                if c.group == Some(group_id) {
                    c.group = parent;
                    count += 1;
                }
            }
            self.groups.retain(|g| g.id != group_id);
        }
        self.selection.groups.clear();
        self.touch();
        count
    }

    pub fn set_curve_group(&mut self, id: CurveId, group: Option<GroupId>) {
        if let Some(c) = self.curves.iter_mut().find(|c| c.id == id) {
            c.group = group;
            self.touch();
        }
    }

    /// A group is effectively visible only when it and every ancestor are.
    pub fn group_effective_visible(&self, group: Option<GroupId>) -> bool {
        let mut current = group;
        let mut guard = 0;
        while let Some(id) = current {
            let Some(g) = self.group(id) else { break };
            if !g.visible {
                return false;
            }
            current = g.parent;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        true
    }

    pub fn group_effective_locked(&self, group: Option<GroupId>) -> bool {
        let mut current = group;
        let mut guard = 0;
        while let Some(id) = current {
            let Some(g) = self.group(id) else { break };
            if g.locked {
                return true;
            }
            current = g.parent;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        false
    }

    pub fn curve_visible(&self, curve: &Curve) -> bool {
        curve.visible && self.group_effective_visible(curve.group)
    }

    pub fn curve_locked(&self, curve: &Curve) -> bool {
        curve.locked || self.group_effective_locked(curve.group)
    }

    /// Build every visible curve mesh. Returns `(curve_id, mesh)` pairs.
    pub fn curve_meshes(&self, quality: MeshQuality) -> Vec<(CurveId, Mesh)> {
        self.curves
            .iter()
            .filter(|c| self.curve_visible(c))
            .map(|c| (c.id, curve_mesh(c, quality)))
            .filter(|(_, m)| !m.is_empty())
            .collect()
    }

    pub fn guide_meshes(&mut self) -> Vec<(GuideId, Mesh, Mat4, Vec4, bool)> {
        self.guides
            .iter_mut()
            .filter(|g| g.visible)
            .map(|g| {
                let mat = g.transform;
                let color = g.color;
                let solid = g.solid;
                (g.id, g.world_mesh(), mat, color, solid)
            })
            .collect()
    }

    pub fn bounds_visible(&mut self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for c in &self.curves {
            if !self.curve_visible(c) || c.points.is_empty() {
                continue;
            }
            let (a, b) = c.bounds();
            min = min.min(a);
            max = max.max(b);
        }
        for g in &mut self.guides {
            if !g.visible {
                continue;
            }
            let (a, b) = g.world_bounds();
            min = min.min(a);
            max = max.max(b);
        }
        if min.x > max.x {
            (Vec3::splat(-100.0), Vec3::splat(100.0))
        } else {
            (min, max)
        }
    }

    /// Bounds of visible curves only, excluding guides. A large stage plane or
    /// construction plan can dominate `bounds_visible`; this gives the explicit
    /// "frame artwork" command a tight view without hiding scene geometry.
    pub fn bounds_curves_visible(&mut self) -> Option<(Vec3, Vec3)> {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for c in &self.curves {
            if !self.curve_visible(c) || c.points.is_empty() {
                continue;
            }
            let (a, b) = c.bounds();
            min = min.min(a);
            max = max.max(b);
        }
        (min.x <= max.x).then_some((min, max))
    }

    pub fn selection_bounds(&mut self) -> Option<(Vec3, Vec3)> {
        let curves: Vec<&Curve> = self
            .curves
            .iter()
            .filter(|c| self.selection.curves.contains(&c.id))
            .collect();
        if curves.is_empty() {
            return None;
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for c in curves {
            let (a, b) = c.bounds();
            min = min.min(a);
            max = max.max(b);
        }
        Some((min, max))
    }

    pub fn selection_centre(&mut self) -> Option<Vec3> {
        self.selection_bounds().map(|(a, b)| (a + b) * 0.5)
    }

    /// Apply a world-space matrix to every selected curve. Locked curves are
    /// untouched and reported by the return value.
    pub fn transform_selection(&mut self, mat: Mat4) -> usize {
        let mut count = 0;
        let selected: Vec<CurveId> = self.selection.curves.iter().copied().collect();
        let locked: Vec<CurveId> = self
            .curves
            .iter()
            .filter(|c| c.locked || self.group_effective_locked(c.group))
            .map(|c| c.id)
            .collect();
        for c in &mut self.curves {
            if selected.contains(&c.id) && !locked.contains(&c.id) {
                c.transform(mat);
                count += 1;
            }
        }
        if count > 0 {
            self.touch();
        }
        count
    }

    pub fn translate_selection(&mut self, delta: Vec3) -> usize {
        self.transform_selection(Mat4::from_translation(delta))
    }

    /// Duplicate mirrored copies of the selected curves and bake them into the
    /// scene. Existing mirrors are tagged so they are not mirrored recursively.
    pub fn mirror_selection(&mut self) -> Vec<CurveId> {
        let axis = self.mirror.axis.normal();
        let point = self.mirror.plane_point();
        let source: Vec<Curve> = self
            .curves
            .iter()
            .filter(|c| self.selection.curves.contains(&c.id))
            .filter(|c| c.tag.as_deref() != Some("mirror"))
            .cloned()
            .collect();
        let mut created = Vec::new();
        for curve in source {
            let id = self.new_curve_id();
            let mut copy = mirror_curve(id, &curve, point, axis);
            copy.tag = Some("mirror".into());
            copy.name = format!("{} mirror", curve.name);
            self.curves.push(copy);
            created.push(id);
        }
        if !created.is_empty() {
            self.selection.curves.extend(created.iter().copied());
            self.touch();
        }
        created
    }

    /// Create the live mirror for a freshly drawn stroke if enabled.
    pub fn mirror_curve_if_enabled(&mut self, curve: &Curve) -> Option<CurveId> {
        if !self.mirror.enabled || !self.mirror.live {
            return None;
        }
        let id = self.new_curve_id();
        let mut copy = mirror_curve(
            id,
            curve,
            self.mirror.plane_point(),
            self.mirror.axis.normal(),
        );
        copy.tag = Some("mirror".into());
        self.curves.push(copy);
        self.touch();
        Some(id)
    }

    /// True when the scene has no authored art.
    pub fn is_empty(&self) -> bool {
        self.curves.is_empty() && self.guides.is_empty() && self.groups.is_empty()
    }

    /// Add a demonstration scene used by "New from gallery".
    pub fn sample_scene(brush: Brush) -> Self {
        let mut scene = Scene::new();
        let marker = Brush {
            base_size: 7.0,
            opacity: 0.9,
            ..brush.clone()
        };
        let mut c1 = Curve::from_positions(
            scene.new_curve_id(),
            vec![
                Vec3::new(-180.0, 32.0, -60.0),
                Vec3::new(-90.0, 92.0, -30.0),
                Vec3::new(0.0, 116.0, 20.0),
                Vec3::new(95.0, 78.0, 60.0),
                Vec3::new(178.0, 22.0, 30.0),
            ],
            0.82,
            marker.clone(),
        )
        .with_name("Ribbon study");
        c1.kind = crate::curve::CurveKind::CatmullRom;
        c1.color = Vec4::new(0.96, 0.46, 0.32, 1.0);
        scene.add_curve(c1);
        let side = Scene::side_guide(scene.new_guide_id(), Vec3::new(0.0, 150.0, 0.0));
        scene.add_guide(side);
        scene
    }

    fn side_guide(id: GuideId, centre: Vec3) -> Guide {
        let mut g = Guide::new(
            id,
            "Side plane",
            GuideKind::Plane {
                width: 520.0,
                height: 340.0,
            },
        );
        g.transform = Mat4::from_translation(centre)
            * Mat4::from_quat(crate::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
        g.color = Vec4::new(0.42, 0.70, 0.82, 1.0);
        g.opacity = 0.28;
        g.rebuild();
        g
    }

    /// Validate all authored data. Returns a list of problems instead of
    /// failing on the first, so the UI can show a useful report.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for c in &self.curves {
            if c.id == 0 {
                issues.push(format!("curve {} has id 0", c.name));
            }
            if c.points.is_empty() {
                issues.push(format!("curve {} has no points", c.name));
            }
            if !c.points.iter().all(|p| p.position.is_finite()) {
                issues.push(format!("curve {} has non-finite points", c.name));
            }
            if let Some(group) = c.group {
                if self.group(group).is_none() {
                    issues.push(format!("curve {} references missing group {group}", c.name));
                }
            }
            if let Err(e) = c.brush.validate() {
                issues.push(format!("curve {} brush: {e}", c.name));
            }
        }
        for g in &self.guides {
            if g.id == 0 {
                issues.push(format!("guide {} has id 0", g.name));
            }
            if !g.transform.is_finite() {
                issues.push(format!("guide {} has non-finite transform", g.name));
            }
        }
        for g in &self.groups {
            if let Some(parent) = g.parent {
                if parent == g.id || self.group(parent).is_none() {
                    issues.push(format!("group {} has invalid parent", g.name));
                }
            }
        }
        issues
    }
}

/// A complete PLUME document. The scene is ordinary data, so `.plume` files
/// are a JSON manifest plus optional thumbnails and custom assets in a ZIP.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub schema: String,
    pub version: u32,
    pub name: String,
    pub author: String,
    pub notes: String,
    pub created: String,
    pub modified: String,
    pub camera: Camera,
    pub render: RenderSettings,
    pub scene: Scene,
    /// Timeline, camera shots and turntable playback.
    pub animation: Animation,
    #[serde(skip)]
    pub thumbnail: Option<image::RgbaImage>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl Document {
    pub fn new(name: impl Into<String>) -> Self {
        let now = crate::time::utc_timestamp();
        Self {
            schema: PLUME_SCHEMA.into(),
            version: PLUME_FORMAT_VERSION,
            name: name.into(),
            author: "PLUME".into(),
            notes: String::new(),
            created: now.clone(),
            modified: now,
            camera: Camera::default(),
            render: RenderSettings::default(),
            scene: Scene::new(),
            animation: Animation::default(),
            thumbnail: None,
        }
    }

    pub fn with_brush(brush: Brush) -> Self {
        let mut doc = Self::new("Untitled");
        doc.scene = Scene::sample_scene(brush);
        doc
    }

    pub fn mark_modified(&mut self, timestamp: impl Into<String>) {
        self.modified = timestamp.into();
        self.scene.touch();
    }

    pub fn validate(&self) -> Vec<String> {
        let mut issues = self.scene.validate();
        if self.schema != PLUME_SCHEMA {
            issues.push(format!("unknown schema {}", self.schema));
        }
        if self.version > PLUME_FORMAT_VERSION {
            issues.push(format!(
                "document version {} is newer than supported {}",
                self.version, PLUME_FORMAT_VERSION
            ));
        }
        if let Err(e) = self.render.validate() {
            issues.push(format!("render settings: {e}"));
        }
        if let Err(e) = self.animation.validate() {
            issues.push(format!("animation: {e}"));
        }
        if !self.camera.position.is_finite() || !self.camera.target.is_finite() {
            issues.push("camera is not finite".into());
        }
        issues
    }

    pub fn add_camera_shot(&mut self, name: impl Into<String>) {
        let duration = self.animation.effective_duration().max(2.0);
        self.animation.add_shot(name.into(), &self.camera, duration);
    }

    pub fn apply_camera_shot(&mut self, index: usize) {
        if let Some(shot) = self.animation.shots.get(index) {
            shot.pose.apply_to(&mut self.camera);
        }
    }

    pub fn stats(&self) -> DocumentStats {
        let points = self.scene.curves.iter().map(|c| c.points.len()).sum();
        DocumentStats {
            curves: self.scene.curves.len(),
            guides: self.scene.guides.len(),
            groups: self.scene.groups.len(),
            control_points: points,
            camera_shots: self.animation.shots.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocumentStats {
    pub curves: usize,
    pub guides: usize,
    pub groups: usize,
    pub control_points: usize,
    pub camera_shots: usize,
}

#[derive(Clone, Debug)]
struct Snapshot {
    label: String,
    document: Document,
}

/// Bounded snapshot undo. Document clones are cheap relative to GPU state and
/// keep every operator undoable without bespoke inverse code.
#[derive(Debug)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(100)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(2),
        }
    }

    pub fn record(&mut self, label: impl Into<String>, document: &Document) {
        self.push_snapshot(label, document.clone());
    }

    /// Push an already-cloned document as an undo state. The desktop harness
    /// uses this to snapshot the document on pointer-down without cloning twice
    /// when the gesture turns out to be an edit.
    pub fn push_snapshot(&mut self, label: impl Into<String>, document: Document) {
        self.undo.push(Snapshot {
            label: label.into(),
            document,
        });
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn undo(&mut self, current: &mut Document) -> Option<String> {
        let snapshot = self.undo.pop()?;
        self.redo.push(Snapshot {
            label: snapshot.label.clone(),
            document: current.clone(),
        });
        *current = snapshot.document;
        current.thumbnail = None;
        Some(snapshot.label)
    }

    pub fn redo(&mut self, current: &mut Document) -> Option<String> {
        let snapshot = self.redo.pop()?;
        self.undo.push(Snapshot {
            label: snapshot.label.clone(),
            document: current.clone(),
        });
        *current = snapshot.document;
        current.thumbnail = None;
        Some(snapshot.label)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::brush::Brush;
    use crate::curve::Curve;
    use crate::guide::Guide;

    use super::*;

    fn stroke(scene: &mut Scene) -> CurveId {
        let id = scene.new_curve_id();
        let c = Curve::from_positions(
            id,
            vec![Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 20.0],
            0.8,
            Brush::default(),
        );
        scene.add_curve(c)
    }

    #[test]
    fn ids_are_unique_and_monotonic() {
        let mut scene = Scene::new();
        let a = stroke(&mut scene);
        let b = stroke(&mut scene);
        assert_ne!(a, b);
        assert!(scene.next_curve_id > b);
    }

    #[test]
    fn adding_a_guide_keeps_active_flags_in_sync() {
        let mut scene = Scene::new();
        let first = scene.add_guide(Guide::sphere(0, Vec3::Y * 40.0, 10.0));
        let second = scene.add_guide(Guide::sphere(0, Vec3::X * 40.0, 10.0));
        assert_eq!(scene.active_guide, Some(second));
        assert!(!scene.guide(first).unwrap().active);
        assert!(scene.guide(second).unwrap().active);
    }

    #[test]
    fn delete_selection_removes_art() {
        let mut scene = Scene::new();
        let a = stroke(&mut scene);
        scene.selection.only_curve(a);
        assert_eq!(scene.delete_selection(), 1);
        assert!(scene.curves.is_empty());
        assert!(scene.selection.is_empty());
    }

    #[test]
    fn grouping_and_visibility_work() {
        let mut scene = Scene::new();
        let a = stroke(&mut scene);
        scene.selection.only_curve(a);
        let group = scene.group_selection("Group").unwrap();
        assert_eq!(scene.curve(a).unwrap().group, Some(group));
        assert!(scene.group_effective_visible(Some(group)));
        scene.group_mut(group).unwrap().visible = false;
        assert!(!scene.curve_visible(scene.curve(a).unwrap()));
        scene.group_mut(group).unwrap().visible = true;
        scene.group_mut(group).unwrap().locked = true;
        assert!(scene.curve_locked(scene.curve(a).unwrap()));
    }

    #[test]
    fn mirror_selection_adds_tagged_copy() {
        let mut scene = Scene::new();
        let a = stroke(&mut scene);
        scene.selection.only_curve(a);
        scene.mirror.axis = MirrorAxis::X;
        let mirrors = scene.mirror_selection();
        assert_eq!(mirrors.len(), 1);
        let m = scene.curve(mirrors[0]).unwrap();
        assert_eq!(m.tag.as_deref(), Some("mirror"));
        assert!(m.points.iter().all(|p| p.position.x <= 0.0));
    }

    #[test]
    fn live_mirror_respects_switch() {
        let mut scene = Scene::new();
        let brush = Brush::default();
        let c = Curve::line(
            0,
            Vec3::new(4.0, 4.0, 0.0),
            Vec3::new(8.0, 6.0, 0.0),
            brush.clone(),
        );
        assert!(scene.mirror_curve_if_enabled(&c).is_none());
        scene.mirror.enabled = true;
        scene.mirror.live = true;
        scene.mirror.axis = MirrorAxis::Y;
        let id = scene.mirror_curve_if_enabled(&c).unwrap();
        assert!(scene.curve(id).unwrap().points[0].position.y < 0.0);
    }

    #[test]
    fn transform_selection_skips_locked_curves() {
        let mut scene = Scene::new();
        let a = stroke(&mut scene);
        scene.selection.only_curve(a);
        scene.curve_mut(a).unwrap().locked = true;
        assert_eq!(scene.translate_selection(Vec3::Y * 10.0), 0);
        scene.curve_mut(a).unwrap().locked = false;
        assert_eq!(scene.translate_selection(Vec3::Y * 10.0), 1);
        assert!((scene.curve(a).unwrap().points[0].position.y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn bounds_include_guides_and_curves() {
        let mut scene = Scene::new();
        stroke(&mut scene);
        scene.add_guide(Guide::sphere(0, Vec3::Y * 40.0, 10.0));
        let (min, max) = scene.bounds_visible();
        assert!(min.y <= 0.01);
        assert!(max.y >= 49.0);
    }

    #[test]
    fn curve_bounds_ignore_stage_guides() {
        let mut scene = Scene::new();
        stroke(&mut scene);
        scene.add_guide(Guide::sphere(0, Vec3::Y * 40.0, 500.0));
        let (_, max) = scene.bounds_visible();
        assert!(
            max.y >= 500.0,
            "the guide should dominate the visible bounds"
        );
        let (min, max) = scene.bounds_curves_visible().unwrap();
        assert!(min.y > -10.0 && max.y < 10.0, "{min:?}..{max:?}");
        assert!(max.x <= 20.0);
    }

    #[test]
    fn curve_bounds_are_absent_without_visible_curves() {
        let mut scene = Scene::new();
        scene.add_guide(Guide::sphere(0, Vec3::ZERO, 10.0));
        assert!(scene.bounds_curves_visible().is_none());
    }

    #[test]
    fn document_roundtrip_json() {
        let mut doc = Document::with_brush(Brush::default());
        doc.add_camera_shot("Hero");
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.scene.curves.len(), doc.scene.curves.len());
        assert_eq!(back.animation.shots.len(), 1);
        assert!(back.validate().is_empty());
    }

    #[test]
    fn history_undo_redo_restores_document() {
        let mut doc = Document::new("Test");
        let mut history = History::new(8);
        history.record("add", &doc);
        stroke(&mut doc.scene);
        history.record("delete", &doc);
        doc.scene.curves.clear();
        assert!(history.undo(&mut doc).is_some());
        assert_eq!(doc.scene.curves.len(), 1);
        assert!(history.undo(&mut doc).is_some());
        assert!(doc.scene.curves.is_empty());
        assert!(history.redo(&mut doc).is_some());
        assert_eq!(doc.scene.curves.len(), 1);
    }

    #[test]
    fn validate_catches_missing_group() {
        let mut doc = Document::new("Bad");
        let id = stroke(&mut doc.scene);
        doc.scene.curve_mut(id).unwrap().group = Some(999);
        assert!(doc.validate().iter().any(|s| s.contains("missing group")));
    }

    #[test]
    fn doc_stats_count_points() {
        let doc = Document::with_brush(Brush::default());
        let stats = doc.stats();
        assert_eq!(stats.curves, 1);
        assert_eq!(stats.guides, 1);
        assert!(stats.control_points >= 5);
    }
}
