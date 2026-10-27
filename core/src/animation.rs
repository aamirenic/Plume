//! Timeline, camera animation and turntable playback.
//!
//! Animation is evaluated on the CPU. The renderer only needs the camera pose
//! for the current time, so scrubbing and export use exactly the same path.

use crate::render::CameraPose;
use crate::viewport::Camera;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    Smooth,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    pub fn label(self) -> &'static str {
        match self {
            Easing::Linear => "Linear",
            Easing::Smooth => "Smooth",
            Easing::EaseIn => "Ease in",
            Easing::EaseOut => "Ease out",
            Easing::EaseInOut => "Ease in-out",
        }
    }

    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::Smooth => t * t * (3.0 - 2.0 * t),
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) * 0.5
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraShot {
    pub name: String,
    pub pose: CameraPose,
    pub duration: f32,
    pub easing: Easing,
}

impl CameraShot {
    pub fn new(name: impl Into<String>, pose: CameraPose, duration: f32) -> Self {
        Self {
            name: name.into(),
            pose,
            duration: duration.max(0.1),
            easing: Easing::Smooth,
        }
    }

    pub fn from_camera(name: impl Into<String>, camera: &Camera, duration: f32) -> Self {
        Self::new(name, CameraPose::from_camera(camera), duration)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKeyframe {
    pub time: f32,
    pub pose: CameraPose,
    pub easing: Easing,
}

impl CameraKeyframe {
    pub fn new(time: f32, pose: CameraPose) -> Self {
        Self {
            time: time.max(0.0),
            pose,
            easing: Easing::Smooth,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurntableAxis {
    Y,
    X,
    Z,
}

impl TurntableAxis {
    pub fn label(self) -> &'static str {
        match self {
            TurntableAxis::Y => "Y (vertical)",
            TurntableAxis::X => "X",
            TurntableAxis::Z => "Z",
        }
    }

    pub fn vector(self) -> crate::Vec3 {
        match self {
            TurntableAxis::Y => crate::Vec3::Y,
            TurntableAxis::X => crate::Vec3::X,
            TurntableAxis::Z => crate::Vec3::Z,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub playing: bool,
    pub time: f32,
    pub speed: f32,
    pub fps: u32,
    pub loop_playback: bool,
    pub duration: f32,
    pub turntable: bool,
    pub turntable_axis: TurntableAxis,
    pub turntable_turns: f32,
    pub motion_blur: f32,
    pub shots: Vec<CameraShot>,
    pub keyframes: Vec<CameraKeyframe>,
    pub transition_seconds: f32,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            playing: false,
            time: 0.0,
            speed: 1.0,
            fps: 30,
            loop_playback: true,
            duration: 12.0,
            turntable: false,
            turntable_axis: TurntableAxis::Y,
            turntable_turns: 1.0,
            motion_blur: 0.0,
            shots: Vec::new(),
            keyframes: Vec::new(),
            transition_seconds: 1.2,
        }
    }
}

impl Animation {
    pub fn play(&mut self) {
        self.playing = true;
        if self.time >= self.effective_duration() {
            self.time = 0.0;
        }
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.time = 0.0;
    }

    pub fn seek(&mut self, time: f32) {
        self.time = time.clamp(0.0, self.effective_duration());
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if !self.playing || dt <= 0.0 {
            return false;
        }
        let duration = self.effective_duration();
        self.time += dt * self.speed.max(0.0);
        if self.time >= duration {
            if self.loop_playback {
                self.time %= duration.max(1.0e-4);
            } else {
                self.time = duration;
                self.playing = false;
            }
        }
        true
    }

    pub fn effective_duration(&self) -> f32 {
        let shots = self.shots.iter().map(|s| s.duration.max(0.1)).sum::<f32>();
        let keys = self.keyframes.iter().map(|k| k.time).fold(0.0f32, f32::max);
        shots.max(keys).max(self.duration.max(0.1)).max(0.1)
    }

    pub fn progress(&self) -> f32 {
        (self.time / self.effective_duration()).clamp(0.0, 1.0)
    }

    pub fn add_shot(&mut self, name: impl Into<String>, camera: &Camera, duration: f32) -> usize {
        self.shots
            .push(CameraShot::from_camera(name, camera, duration));
        self.shots.len() - 1
    }

    pub fn add_keyframe(&mut self, time: f32, camera: &Camera) -> usize {
        self.keyframes
            .push(CameraKeyframe::new(time, CameraPose::from_camera(camera)));
        self.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.keyframes.len() - 1
    }

    /// Total time of all shots, used to lay out the timeline.
    pub fn shot_start(&self, index: usize) -> f32 {
        self.shots
            .iter()
            .take(index)
            .map(|s| s.duration.max(0.1))
            .sum()
    }

    pub fn shot_at_time(&self, time: f32) -> Option<(usize, f32)> {
        if self.shots.is_empty() {
            return None;
        }
        let mut start = 0.0;
        for (i, shot) in self.shots.iter().enumerate() {
            let end = start + shot.duration.max(0.1);
            if time <= end || i + 1 == self.shots.len() {
                return Some((i, ((time - start) / shot.duration.max(0.1)).clamp(0.0, 1.0)));
            }
            start = end;
        }
        Some((self.shots.len() - 1, 1.0))
    }

    /// Evaluate the timeline. `base` is the editor camera; when no keyframes or
    /// shots exist the base pose is returned unchanged, which makes the
    /// animation panel feel like a camera rig rather than a mode switch.
    pub fn sample_pose(&self, base: &CameraPose, time: f32) -> CameraPose {
        let mut pose = if !self.keyframes.is_empty() {
            self.sample_keyframes(base, time)
        } else if !self.shots.is_empty() {
            self.sample_shots(base, time)
        } else {
            base.clone()
        };
        if self.turntable {
            let duration = self.effective_duration();
            let t = if duration > 1.0e-5 {
                (time / duration).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let angle = t * self.turntable_turns * std::f32::consts::TAU;
            let axis = self.turntable_axis.vector();
            let q = crate::Quat::from_axis_angle(axis, angle);
            pose.position = pose.target + q * (pose.position - pose.target);
        }
        pose
    }

    fn sample_keyframes(&self, base: &CameraPose, time: f32) -> CameraPose {
        let keys = &self.keyframes;
        if keys.is_empty() {
            return base.clone();
        }
        if time <= keys[0].time {
            return keys[0].pose.clone();
        }
        for pair in keys.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            if time <= b.time {
                let span = (b.time - a.time).max(1.0e-5);
                let t = ((time - a.time) / span).clamp(0.0, 1.0);
                return a.pose.lerp(&b.pose, b.easing.apply(t));
            }
        }
        keys.last().unwrap().pose.clone()
    }

    fn sample_shots(&self, base: &CameraPose, time: f32) -> CameraPose {
        let shots = &self.shots;
        if shots.is_empty() {
            return base.clone();
        }
        let mut start = 0.0;
        for (i, shot) in shots.iter().enumerate() {
            let end = start + shot.duration.max(0.1);
            if time <= end || i + 1 == shots.len() {
                let local = ((time - start) / shot.duration.max(0.1)).clamp(0.0, 1.0);
                let eased = shot.easing.apply(local);
                if i + 1 < shots.len() && time > end - self.transition_seconds.max(0.0) {
                    let next = &shots[i + 1];
                    let blend = ((time - (end - self.transition_seconds.max(0.0)))
                        / self.transition_seconds.max(1.0e-4))
                    .clamp(0.0, 1.0);
                    return shot.pose.lerp(&next.pose, blend * eased.max(blend));
                }
                return shot.pose.clone();
            }
            start = end;
        }
        shots.last().unwrap().pose.clone()
    }

    pub fn camera_at(&self, base: &Camera, time: f32) -> Camera {
        let pose = self.sample_pose(&CameraPose::from_camera(base), time);
        let mut camera = base.clone();
        pose.apply_to(&mut camera);
        camera
    }

    pub fn remove_shot(&mut self, index: usize) {
        if index < self.shots.len() {
            self.shots.remove(index);
        }
    }

    pub fn remove_keyframe(&mut self, index: usize) {
        if index < self.keyframes.len() {
            self.keyframes.remove(index);
        }
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if !(self.fps >= 1 && self.fps <= 240) {
            return Err("fps must be between 1 and 240".into());
        }
        if !(self.speed.is_finite() && self.speed >= 0.0) {
            return Err("speed must be finite and >= 0".into());
        }
        if !(self.duration.is_finite() && self.duration > 0.0) {
            return Err("duration must be finite and > 0".into());
        }
        if !self.time.is_finite() {
            return Err("time must be finite".into());
        }
        Ok(())
    }

    pub fn frame_count(&self) -> u32 {
        (self.effective_duration() * self.fps as f32)
            .round()
            .max(1.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::Camera;

    fn camera() -> Camera {
        Camera::default()
    }

    #[test]
    fn easing_is_bounded() {
        for e in [
            Easing::Linear,
            Easing::Smooth,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert!((e.apply(0.0) - 0.0).abs() < 1e-5);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-5);
            assert!((0.0..=1.0).contains(&e.apply(0.37)));
        }
    }

    #[test]
    fn turntable_keeps_distance() {
        let mut a = Animation {
            turntable: true,
            turntable_turns: 1.0,
            duration: 10.0,
            ..Animation::default()
        };
        let base = camera();
        let d0 = (base.position - base.target).length();
        let p = a.sample_pose(&CameraPose::from_camera(&base), 2.5);
        assert!(((p.position - p.target).length() - d0).abs() < 1e-3);
        a.turntable_axis = TurntableAxis::X;
        let p = a.sample_pose(&CameraPose::from_camera(&base), 5.0);
        assert!(((p.position - p.target).length() - d0).abs() < 1e-3);
    }

    #[test]
    fn keyframes_interpolate() {
        let base = Camera::default();
        let mut a = Animation::default();
        a.add_keyframe(0.0, &base);
        let mut b = base.clone();
        b.position += crate::Vec3::X * 10.0;
        a.add_keyframe(10.0, &b);
        let p = a.sample_pose(&CameraPose::from_camera(&base), 5.0);
        assert!((p.position.x - (base.position.x + 5.0)).abs() < 1e-3);
    }

    #[test]
    fn playback_loops_and_stops() {
        let mut a = Animation {
            duration: 1.0,
            loop_playback: true,
            ..Animation::default()
        };
        a.play();
        a.tick(1.5);
        assert!(a.time <= 1.0);
        assert!(a.playing);
        a.loop_playback = false;
        a.seek(0.9);
        a.tick(0.2);
        assert!(!a.playing);
        assert!((a.time - 1.0).abs() < 1e-5);
    }

    #[test]
    fn shots_are_sequential() {
        let base = Camera::default();
        let mut a = Animation {
            duration: 4.0,
            ..Animation::default()
        };
        a.add_shot("A", &base, 1.0);
        let mut b = base.clone();
        b.target += crate::Vec3::X * 20.0;
        a.add_shot("B", &b, 2.0);
        let p = a.sample_pose(&CameraPose::from_camera(&base), 1.5);
        assert!(p.target.x > 5.0);
    }

    #[test]
    fn progress_and_frame_count() {
        let a = Animation {
            duration: 2.0,
            fps: 24,
            ..Animation::default()
        };
        assert_eq!(a.frame_count(), 48);
        assert!((a.progress() - 0.0).abs() < 1e-5);
    }

    #[test]
    fn invalid_animation_rejected() {
        let mut a = Animation {
            fps: 0,
            ..Animation::default()
        };
        assert!(a.validate().is_err());
        a.fps = 24;
        a.duration = -2.0;
        assert!(a.validate().is_err());
    }
}
