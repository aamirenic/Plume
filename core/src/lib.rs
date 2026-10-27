//! PLUME core — a 3D curve sketching engine.
//!
//! The crate is deliberately UI-agnostic.  The desktop dev harness and the
//! Android shell both drive the same types and algorithms.
//!
//! Coordinate system: right handed, **Y up**, ground plane on XZ.
//! Units: millimetres by convention; the renderer maps a unit to one world
//! millimetre and fits the camera distance to the scene.

pub mod animation;
pub mod brush;
pub mod bvh;
pub mod capture;
pub mod curve;
pub mod extrusion;
pub mod gallery;
pub mod guide;
pub mod io;
pub mod liquify;
pub mod math;
pub mod mesh;
pub mod pattern;
pub mod render;
pub mod rmf;
pub mod scene;
pub mod selection;
pub mod shapes;
pub mod time;
pub mod transform;
pub mod viewport;

pub use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

/// Result alias used by all fallible PLUME operations.
pub type Result<T> = std::result::Result<T, PlumeError>;

/// Errors surfaced by the core.
///
/// Wrapped errors are rendered into the message but are deliberately not
/// exposed as [`std::error::Error::source`]. Callers such as `anyhow` already
/// print the full chain when asked for it, so exposing the same text twice
/// would turn one filesystem failure into a duplicated CLI message.
#[derive(Debug, thiserror::Error)]
pub enum PlumeError {
    #[error("io: {0}")]
    Io(std::io::Error),
    #[error("json: {0}")]
    Json(serde_json::Error),
    #[error("zip: {0}")]
    Zip(zip::result::ZipError),
    #[error("image: {0}")]
    Image(image::ImageError),
    #[error("gltf: {0}")]
    Gltf(gltf::Error),
    #[error("obj: {0}")]
    Obj(String),
    #[error("invalid file: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("validation: {0}")]
    Validation(String),
}

macro_rules! plume_error_from {
    ($type:ty, $variant:ident) => {
        impl From<$type> for PlumeError {
            fn from(error: $type) -> Self {
                PlumeError::$variant(error)
            }
        }
    };
}

plume_error_from!(std::io::Error, Io);
plume_error_from!(serde_json::Error, Json);
plume_error_from!(zip::result::ZipError, Zip);
plume_error_from!(image::ImageError, Image);
plume_error_from!(gltf::Error, Gltf);

impl From<Box<dyn std::error::Error + Send + Sync>> for PlumeError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        PlumeError::Invalid(e.to_string())
    }
}

pub type CurveId = u64;
pub type GuideId = u64;
pub type GroupId = u64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_errors_are_not_reported_twice() {
        let error = PlumeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing file",
        ));
        assert!(std::error::Error::source(&error).is_none());
        assert_eq!(error.to_string(), "io: missing file");
    }
}
