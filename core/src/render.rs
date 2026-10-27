//! Render-facing settings: background, lighting, shadows and post-processing.
//!
//! These types are plain serialisable data. The desktop renderer and the
//! Android shell both translate them into GPU constants, which keeps the
//! document the single source of truth.

use crate::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundKind {
    Solid,
    Gradient,
    Studio,
}

impl BackgroundKind {
    pub fn label(self) -> &'static str {
        match self {
            BackgroundKind::Solid => "Solid",
            BackgroundKind::Gradient => "Gradient",
            BackgroundKind::Studio => "Studio sweep",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Tonemap {
    Linear,
    Reinhard,
    Aces,
    Filmic,
}

impl Tonemap {
    pub fn label(self) -> &'static str {
        match self {
            Tonemap::Linear => "Linear",
            Tonemap::Reinhard => "Reinhard",
            Tonemap::Aces => "ACES",
            Tonemap::Filmic => "Filmic",
        }
    }

    pub fn id(self) -> u32 {
        match self {
            Tonemap::Linear => 0,
            Tonemap::Reinhard => 1,
            Tonemap::Aces => 2,
            Tonemap::Filmic => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Background {
    pub kind: BackgroundKind,
    pub color: Vec4,
    pub top: Vec4,
    pub bottom: Vec4,
    /// Horizontal position of the studio horizon, 0..1.
    pub horizon: f32,
    /// 0 = flat, 1 = strong vignette in the background itself.
    pub vignette: f32,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            kind: BackgroundKind::Studio,
            color: Vec4::new(0.12, 0.128, 0.15, 1.0),
            top: Vec4::new(0.235, 0.245, 0.275, 1.0),
            bottom: Vec4::new(0.055, 0.06, 0.075, 1.0),
            horizon: 0.52,
            vignette: 0.22,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    pub background: Background,
    /// Multiplier on image-less studio reflections.
    pub intensity: f32,
    /// Rotation of the environment in degrees.
    pub rotation_deg: f32,
    /// Colour bounce from the ground.
    pub ground_bounce: Vec4,
    /// Colour bounce from the sky.
    pub sky_bounce: Vec4,
    /// World-space ground plane height.
    pub ground_height: f32,
    pub ground_color: Vec4,
    pub ground_roughness: f32,
    pub ground_shadow: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            background: Background::default(),
            intensity: 1.0,
            rotation_deg: 35.0,
            ground_bounce: Vec4::new(0.16, 0.17, 0.19, 1.0),
            sky_bounce: Vec4::new(0.58, 0.64, 0.74, 1.0),
            ground_height: 0.0,
            ground_color: Vec4::new(0.085, 0.089, 0.098, 1.0),
            ground_roughness: 0.78,
            ground_shadow: 0.65,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

impl LightKind {
    pub fn label(self) -> &'static str {
        match self {
            LightKind::Directional => "Directional",
            LightKind::Point => "Point",
            LightKind::Spot => "Spot",
        }
    }

    pub fn shader_id(self) -> u32 {
        match self {
            LightKind::Directional => 0,
            LightKind::Point => 1,
            LightKind::Spot => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Light {
    pub id: u64,
    pub name: String,
    pub kind: LightKind,
    pub enabled: bool,
    pub position: Vec3,
    /// Direction the light points towards.
    pub direction: Vec3,
    pub color: Vec4,
    pub intensity: f32,
    pub range: f32,
    /// Full cone angle in degrees (spot lights).
    pub cone_deg: f32,
    pub cast_shadow: bool,
    pub shadow_bias: f32,
    pub softness: f32,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            id: 1,
            name: "Key".into(),
            kind: LightKind::Directional,
            enabled: true,
            position: Vec3::new(160.0, 220.0, 120.0),
            direction: Vec3::new(-0.55, -0.72, -0.42).normalize(),
            color: Vec4::new(1.0, 0.96, 0.90, 1.0),
            intensity: 3.4,
            range: 600.0,
            cone_deg: 38.0,
            cast_shadow: true,
            shadow_bias: 0.0015,
            softness: 1.4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lighting {
    pub lights: Vec<Light>,
    pub ambient_intensity: f32,
    pub ambient_color: Vec4,
    pub exposure: f32,
    pub shadows: bool,
    pub shadow_resolution: u32,
    pub shadow_distance: f32,
    pub rim_strength: f32,
    pub specular: f32,
    pub studio_reflections: bool,
    pub next_light_id: u64,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            lights: vec![
                Light::default(),
                Light {
                    id: 2,
                    name: "Fill".into(),
                    kind: LightKind::Directional,
                    enabled: true,
                    position: Vec3::new(-210.0, 90.0, 170.0),
                    direction: Vec3::new(0.72, -0.28, -0.64).normalize(),
                    color: Vec4::new(0.56, 0.68, 0.95, 1.0),
                    intensity: 1.15,
                    range: 600.0,
                    cone_deg: 42.0,
                    cast_shadow: false,
                    shadow_bias: 0.0015,
                    softness: 2.0,
                },
                Light {
                    id: 3,
                    name: "Rim".into(),
                    kind: LightKind::Directional,
                    enabled: true,
                    position: Vec3::new(-40.0, 160.0, -260.0),
                    direction: Vec3::new(0.15, -0.36, 0.92).normalize(),
                    color: Vec4::new(0.95, 0.80, 0.62, 1.0),
                    intensity: 1.8,
                    range: 600.0,
                    cone_deg: 35.0,
                    cast_shadow: false,
                    shadow_bias: 0.0015,
                    softness: 2.0,
                },
            ],
            ambient_intensity: 0.30,
            ambient_color: Vec4::new(0.32, 0.37, 0.48, 1.0),
            exposure: 1.0,
            shadows: true,
            shadow_resolution: 2048,
            shadow_distance: 900.0,
            rim_strength: 0.18,
            specular: 0.35,
            studio_reflections: true,
            next_light_id: 4,
        }
    }
}

impl Lighting {
    pub fn add_light(&mut self, kind: LightKind) -> u64 {
        let id = self.next_light_id.max(1);
        self.next_light_id = id + 1;
        let mut light = Light {
            id,
            name: format!("{} light", kind.label()),
            kind,
            ..Light::default()
        };
        light.cast_shadow = false;
        self.lights.push(light);
        id
    }

    pub fn remove_light(&mut self, id: u64) -> bool {
        let before = self.lights.len();
        self.lights.retain(|l| l.id != id);
        self.lights.len() != before
    }

    pub fn light(&self, id: u64) -> Option<&Light> {
        self.lights.iter().find(|l| l.id == id)
    }

    pub fn light_mut(&mut self, id: u64) -> Option<&mut Light> {
        self.lights.iter_mut().find(|l| l.id == id)
    }

    /// First enabled shadow-casting light.
    pub fn shadow_light(&self) -> Option<&Light> {
        self.lights
            .iter()
            .find(|l| l.enabled && l.cast_shadow && l.intensity > 0.0)
    }

    pub fn set_only_shadow(&mut self, id: u64) {
        for l in &mut self.lights {
            l.cast_shadow = l.id == id;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FogMode {
    None,
    Linear,
    Exponential,
}

impl FogMode {
    pub fn label(self) -> &'static str {
        match self {
            FogMode::None => "Off",
            FogMode::Linear => "Linear",
            FogMode::Exponential => "Exponential",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PostFx {
    pub exposure: f32,
    pub tonemap: Tonemap,
    pub saturation: f32,
    pub contrast: f32,
    pub lift: f32,
    pub bloom: f32,
    pub bloom_threshold: f32,
    pub bloom_radius: f32,
    pub bloom_soft_knee: f32,
    pub vignette: f32,
    pub grain: f32,
    pub grain_animated: bool,
    pub chromatic_aberration: f32,
    pub sharpen: f32,
    pub fog: FogMode,
    pub fog_color: Vec4,
    pub fog_density: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    /// Screen-space depth of field strength. 0 disables the effect.
    pub depth_of_field: f32,
    pub focus_distance: f32,
    pub outline: f32,
}

impl Default for PostFx {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            tonemap: Tonemap::Aces,
            saturation: 1.06,
            contrast: 1.02,
            lift: 0.0,
            bloom: 0.22,
            bloom_threshold: 0.85,
            bloom_radius: 1.0,
            bloom_soft_knee: 0.35,
            vignette: 0.28,
            grain: 0.004,
            grain_animated: true,
            chromatic_aberration: 0.12,
            sharpen: 0.25,
            fog: FogMode::None,
            fog_color: Vec4::new(0.07, 0.08, 0.1, 1.0),
            fog_density: 0.0012,
            fog_start: 120.0,
            fog_end: 1400.0,
            depth_of_field: 0.0,
            focus_distance: 280.0,
            outline: 0.0,
        }
    }
}

impl PostFx {
    /// Combined exposure used by the renderer. Negative values may be used to
    /// darken, so this is additive in stops.
    pub fn exposure_multiplier(&self, lighting_exposure: f32) -> f32 {
        2.0f32.powf(lighting_exposure + self.exposure)
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.exposure.is_finite() {
            return Err("post exposure must be finite".into());
        }
        if self.saturation < 0.0 || !self.saturation.is_finite() {
            return Err("saturation must be >= 0".into());
        }
        if self.bloom < 0.0 || !self.bloom.is_finite() {
            return Err("bloom must be >= 0".into());
        }
        if self.vignette < 0.0 {
            return Err("vignette must be >= 0".into());
        }
        if self.fog_density < 0.0 {
            return Err("fog density must be >= 0".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderSettings {
    /// Show the XZ construction grid.
    pub grid: bool,
    pub grid_extent: f32,
    pub grid_spacing: f32,
    pub grid_major_every: u32,
    pub grid_color: Vec4,
    pub grid_major_color: Vec4,
    pub axes: bool,
    pub origin_gizmo: bool,
    /// Draw curves as wireframe tubes.
    pub wireframe: bool,
    pub curve_outlines: bool,
    pub guide_fill: bool,
    pub guide_wire: bool,
    /// Render at half resolution while navigating.
    pub dynamic_resolution: bool,
    pub background: Background,
    pub environment: Environment,
    pub lighting: Lighting,
    pub postfx: PostFx,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            grid: true,
            grid_extent: 2400.0,
            grid_spacing: 20.0,
            grid_major_every: 5,
            grid_color: Vec4::new(0.20, 0.22, 0.26, 0.35),
            grid_major_color: Vec4::new(0.31, 0.34, 0.40, 0.6),
            axes: true,
            origin_gizmo: true,
            wireframe: false,
            curve_outlines: true,
            guide_fill: false,
            guide_wire: true,
            dynamic_resolution: true,
            background: Background::default(),
            environment: Environment::default(),
            lighting: Lighting::default(),
            postfx: PostFx::default(),
        }
    }
}

impl RenderSettings {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !(self.grid_extent.is_finite() && self.grid_extent > 0.0) {
            return Err("grid extent must be finite and > 0".into());
        }
        if !(self.grid_spacing.is_finite() && self.grid_spacing > 0.0) {
            return Err("grid spacing must be finite and > 0".into());
        }
        self.lighting.lights.iter().try_for_each(|l| {
            if !(l.intensity.is_finite() && l.intensity >= 0.0) {
                return Err(format!("light {} has invalid intensity", l.name));
            }
            if l.direction.length_squared() < 1.0e-8 && !matches!(l.kind, LightKind::Point) {
                return Err(format!("light {} has no direction", l.name));
            }
            Ok(())
        })?;
        self.postfx.validate()
    }
}

/// A named camera composition. Shots are stored in the document and can be
/// exported as animation segments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraPose {
    pub position: Vec3,
    pub target: Vec3,
    pub fov_y_deg: f32,
    pub roll_deg: f32,
}

impl CameraPose {
    pub fn from_camera(camera: &crate::viewport::Camera) -> Self {
        Self {
            position: camera.position,
            target: camera.target,
            fov_y_deg: camera.fov_y_deg,
            roll_deg: camera.roll_deg,
        }
    }

    pub fn apply_to(&self, camera: &mut crate::viewport::Camera) {
        camera.position = self.position;
        camera.target = self.target;
        camera.fov_y_deg = self.fov_y_deg.clamp(10.0, 120.0);
        camera.roll_deg = self.roll_deg;
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            position: self.position.lerp(other.position, t),
            target: self.target.lerp(other.target, t),
            fov_y_deg: self.fov_y_deg + (other.fov_y_deg - self.fov_y_deg) * t,
            roll_deg: self.roll_deg + (other.roll_deg - self.roll_deg) * t,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::{Camera, ViewSnap};

    #[test]
    fn default_render_settings_validate() {
        RenderSettings::default().validate().unwrap();
    }

    #[test]
    fn lights_are_manageable() {
        let mut lighting = Lighting::default();
        let count = lighting.lights.len();
        let id = lighting.add_light(LightKind::Point);
        assert_eq!(lighting.lights.len(), count + 1);
        assert!(lighting.light(id).is_some());
        lighting.light_mut(id).unwrap().intensity = 8.0;
        assert!(lighting.remove_light(id));
        assert_eq!(lighting.lights.len(), count);
    }

    #[test]
    fn shadow_light_prefers_enabled_casters() {
        let mut lighting = Lighting::default();
        let id = lighting.lights[0].id;
        assert_eq!(lighting.shadow_light().unwrap().id, id);
        lighting.lights[0].cast_shadow = false;
        assert!(lighting.shadow_light().is_none());
        lighting.set_only_shadow(id);
        assert_eq!(lighting.shadow_light().unwrap().id, id);
    }

    #[test]
    fn postfx_exposure_is_additive() {
        let p = PostFx {
            exposure: 1.0,
            ..PostFx::default()
        };
        assert!((p.exposure_multiplier(0.0) - 2.0).abs() < 1e-5);
        assert!((p.exposure_multiplier(-1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn camera_pose_roundtrip() {
        let mut camera = Camera::default();
        camera.snap_to(ViewSnap::Front);
        let pose = CameraPose::from_camera(&camera);
        let mut other = Camera::default();
        pose.apply_to(&mut other);
        assert!((other.position - camera.position).length() < 1e-5);
        assert!((other.fov_y_deg - camera.fov_y_deg).abs() < 1e-5);
    }

    #[test]
    fn pose_interpolation_hits_midpoint() {
        let a = CameraPose {
            position: Vec3::ZERO,
            target: Vec3::Z,
            fov_y_deg: 30.0,
            roll_deg: 0.0,
        };
        let b = CameraPose {
            position: Vec3::X * 10.0,
            target: Vec3::new(0.0, 0.0, 20.0),
            fov_y_deg: 60.0,
            roll_deg: 90.0,
        };
        let m = a.lerp(&b, 0.5);
        assert!((m.position.x - 5.0).abs() < 1e-5);
        assert!((m.fov_y_deg - 45.0).abs() < 1e-5);
        assert!((m.roll_deg - 45.0).abs() < 1e-5);
    }

    #[test]
    fn invalid_values_are_rejected() {
        let p = PostFx {
            saturation: -1.0,
            ..PostFx::default()
        };
        assert!(p.validate().is_err());
        let r = RenderSettings {
            grid_spacing: 0.0,
            ..RenderSettings::default()
        };
        assert!(r.validate().is_err());
    }
}
