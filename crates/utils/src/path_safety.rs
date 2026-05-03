use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPath(PathBuf);

impl CanonicalPath {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, PathSafetyError> {
        let path = path.as_ref();
        dunce::canonicalize(path)
            .map(Self)
            .map_err(|source| PathSafetyError::Canonicalize {
                path: path.to_path_buf(),
                source,
            })
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

#[derive(Debug, Error)]
pub enum PathSafetyError {
    #[error("failed to canonicalize path {}: {source}", path.display())]
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("path {} is not under root {}", path.display(), root.display())]
    OutsideRoot { path: PathBuf, root: PathBuf },
}

pub fn assert_path_under_root(
    path: impl AsRef<Path>,
    root: impl AsRef<Path>,
) -> Result<CanonicalPath, PathSafetyError> {
    let path = CanonicalPath::new(path)?;
    let root = CanonicalPath::new(root)?;

    path.as_path()
        .strip_prefix(root.as_path())
        .map_err(|_| PathSafetyError::OutsideRoot {
            path: path.as_path().to_path_buf(),
            root: root.as_path().to_path_buf(),
        })?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::*;

    fn temp_test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vk-path-safety-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn allows_path_under_root_after_canonicalization() {
        let root = temp_test_dir();
        let child = root.join("child");
        fs::create_dir_all(&child).unwrap();

        let asserted = assert_path_under_root(child.join("..").join("child"), &root).unwrap();

        assert_eq!(asserted.as_path(), dunce::canonicalize(&child).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_path_outside_root() {
        let root = temp_test_dir();
        let outside = temp_test_dir();

        let err = assert_path_under_root(&outside, &root).unwrap_err();

        assert!(matches!(err, PathSafetyError::OutsideRoot { .. }));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = temp_test_dir();
        let outside = temp_test_dir();
        let link = root.join("escape");
        symlink(&outside, &link).unwrap();

        let err = assert_path_under_root(&link, &root).unwrap_err();

        assert!(matches!(err, PathSafetyError::OutsideRoot { .. }));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
