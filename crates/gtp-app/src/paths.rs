//! Path safety (docs/specification.md section 9): every file the app reads or writes inside a
//! library is named by a path relative to the library root, and must stay inside it, also through
//! links. Exports may also go to a folder the user picked.

use std::path::{Component, Path, PathBuf};

use gtp_core::{AppError, ErrorCode};

fn escape(path: &Path) -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "That path points outside the allowed folder.",
    )
    .with_details(path.display().to_string())
}

/// The canonical form of an existing directory.
pub fn canonical_dir(path: &Path) -> Result<PathBuf, AppError> {
    let canonical = path.canonicalize().map_err(|err| AppError::from_io(&err))?;
    if !canonical.is_dir() {
        return Err(
            AppError::new(ErrorCode::InvalidInput, "That is not a folder.")
                .with_details(path.display().to_string()),
        );
    }
    Ok(canonical)
}

/// The canonical form of `path`'s nearest existing ancestor (the path itself when it exists).
fn canonical_existing_part(path: &Path) -> Result<PathBuf, AppError> {
    let mut probe = path;
    loop {
        if probe.exists() {
            return probe.canonicalize().map_err(|err| AppError::from_io(&err));
        }
        probe = probe.parent().ok_or_else(|| escape(path))?;
    }
}

/// Resolves `rel`, a path relative to `root`, to an absolute path inside `root`. Absolute paths,
/// drive prefixes and `..` are refused, and so is a link (anywhere along the existing part of the
/// path) that leads outside `root`. The path itself need not exist yet.
pub fn resolve_inside(root: &Path, rel: &Path) -> Result<PathBuf, AppError> {
    let plain = rel
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !plain || rel.as_os_str().is_empty() {
        return Err(escape(rel));
    }
    let root = canonical_dir(root)?;
    let joined = root.join(rel);
    if !canonical_existing_part(&joined)?.starts_with(&root) {
        return Err(escape(rel));
    }
    Ok(joined)
}

/// Checks that `path` (absolute, need not exist) lies inside one of `roots`; returns it as given.
pub fn check_inside_any(path: &Path, roots: &[&Path]) -> Result<PathBuf, AppError> {
    if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return Err(escape(path));
    }
    let existing = canonical_existing_part(path)?;
    for root in roots {
        if existing.starts_with(canonical_dir(root)?) {
            return Ok(path.to_path_buf());
        }
    }
    Err(escape(path))
}

/// A path for people to read: Windows' verbatim `\\?\C:\...` prefix from `canonicalize` removed.
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_owned(),
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gtp-paths-{label}-{}-{}",
            std::process::id(),
            gtp_core::FrameId::new()
        ));
        std::fs::create_dir_all(dir.join("originals")).unwrap();
        dir
    }

    #[test]
    fn plain_relative_paths_resolve_inside_the_root() {
        let root = temp_root("ok");
        let p = resolve_inside(&root, Path::new("originals/roll/frame/scan.tif")).unwrap();
        assert!(p.starts_with(root.canonicalize().unwrap()));
        assert!(p.ends_with("originals/roll/frame/scan.tif"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn escapes_are_refused() {
        let root = temp_root("escape");
        for bad in ["../outside.tif", "originals/../../x", "", "/etc/passwd"] {
            let err = resolve_inside(&root, Path::new(bad)).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        }
        #[cfg(windows)]
        for bad in [r"C:\Windows\x", r"\\server\share\x", r"C:x"] {
            assert!(resolve_inside(&root, Path::new(bad)).is_err(), "{bad:?}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_link_leading_outside_is_refused() {
        let root = temp_root("link");
        let outside = temp_root("outside");
        let link = root.join("originals").join("sneaky");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link).is_ok();
        // Windows needs a privilege or developer mode for symlinks; a junction-free check is
        // skipped there when the link cannot be made.
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&outside, &link).is_ok();
        if made {
            let err = resolve_inside(&root, Path::new("originals/sneaky/x.tif")).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn export_targets_must_be_inside_an_allowed_folder() {
        let library = temp_root("lib");
        let picked = temp_root("picked");
        let elsewhere = temp_root("elsewhere");
        let roots = [library.as_path(), picked.as_path()];
        assert!(check_inside_any(&picked.join("new/a.jpg"), &roots).is_ok());
        assert!(check_inside_any(&library.join("exports/a.jpg"), &roots).is_ok());
        assert!(check_inside_any(&elsewhere.join("a.jpg"), &roots).is_err());
        assert!(check_inside_any(&picked.join("../x.jpg"), &roots).is_err());
        assert!(check_inside_any(Path::new("relative.jpg"), &roots).is_err());
        for dir in [library, picked, elsewhere] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn display_paths_drop_the_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\C:\Film\Lib")), r"C:\Film\Lib");
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share")),
            r"\\?\UNC\server\share"
        );
        assert_eq!(display_path(Path::new("/Users/me/Film")), "/Users/me/Film");
    }
}
