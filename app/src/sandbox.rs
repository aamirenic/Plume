//! Filesystem sandbox.
//!
//! PLUME never writes outside one explicitly chosen root.  The desktop build
//! defaults to the parent of the Cargo workspace (the authoring sandbox used for
//! development) and can be pointed anywhere with `PLUME_SANDBOX`.  Every file
//! dialog result is validated here before it reaches the core IO layer.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Hard boundary for all user file operations.
#[derive(Clone, Debug)]
pub struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// Resolve the sandbox from `PLUME_SANDBOX` or the workspace parent.
    pub fn discover() -> Result<Self> {
        let root = if let Some(value) = std::env::var_os("PLUME_SANDBOX") {
            PathBuf::from(value)
        } else {
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            manifest
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        };
        Self::at(root)
    }

    /// Build a sandbox rooted at `root`, creating the directory if necessary.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .with_context(|| format!("could not create sandbox {}", root.display()))?;
        let root = root
            .canonicalize()
            .with_context(|| format!("could not resolve sandbox {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("local_projects")
    }

    /// Validate and create the parent of a destination file.  The returned path
    /// is absolute, canonical in its parent component, and guaranteed to be
    /// inside the sandbox.
    pub fn ensure_file(&self, path: &Path) -> Result<PathBuf> {
        let absolute = self.absolute(path);
        if absolute.is_dir() {
            bail!(
                "destination is a folder; choose a file name\n{}",
                absolute.display()
            );
        }
        let parent = absolute.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create folder {}", parent.display()))?;
        let parent = parent
            .canonicalize()
            .with_context(|| format!("could not resolve folder {}", parent.display()))?;
        if !parent.starts_with(&self.root) {
            bail!(
                "refusing to write outside the PLUME sandbox\n{}",
                parent.display()
            );
        }
        let name = absolute
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("destination has no file name"))?;
        Ok(parent.join(name))
    }

    /// True when an existing or prospective path is inside the sandbox.
    pub fn contains(&self, path: &Path) -> bool {
        let absolute = self.absolute(path);
        let probe = existing_ancestor(&absolute);
        match probe.canonicalize() {
            Ok(canonical) => canonical.starts_with(&self.root),
            Err(_) => false,
        }
    }

    fn absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }
}

fn existing_ancestor(path: &Path) -> &Path {
    let mut current = path;
    while !current.exists() {
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => current = parent,
            _ => return path,
        }
    }
    current
}

/// Windows canonicalisation returns verbatim paths (`\\?\C:\...`).  They are
/// correct for I/O but noisy in labels and destination fields.
pub fn pretty_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

/// Make a name safe for use as a single file stem.
pub fn sanitize_stem(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ' ') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim().trim_matches('.').trim();
    // Punctuation-only names would otherwise turn into a stem such as `___`,
    // which is valid on disk but meaningless in the project list.
    if trimmed.is_empty() || !trimmed.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        "Untitled".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("plume-sandbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn accepts_files_inside_the_root() {
        let root = temp_root("inside");
        let sandbox = Sandbox::at(&root).unwrap();
        let path = sandbox
            .ensure_file(&sandbox.projects_dir().join("a.plume"))
            .unwrap();
        assert!(path.starts_with(sandbox.root()));
        assert_eq!(path.file_name().unwrap(), "a.plume");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_escape_paths() {
        let root = temp_root("escape");
        let sandbox = Sandbox::at(&root).unwrap();
        let outside = sandbox.root().join("..").join("definitely-outside.plume");
        assert!(sandbox.ensure_file(&outside).is_err());
        assert!(!sandbox.contains(&outside));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_existing_folder_as_file_destination() {
        let root = temp_root("folder");
        let sandbox = Sandbox::at(&root).unwrap();
        let error = sandbox.ensure_file(sandbox.root()).unwrap_err();
        assert!(
            format!("{error:#}").contains("destination is a folder"),
            "{error:#}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_stem("Ribbon: bloom / 01"), "Ribbon_ bloom _ 01");
        assert_eq!(sanitize_stem(" ... "), "Untitled");
        assert_eq!(sanitize_stem("study-1"), "study-1");
    }
}
