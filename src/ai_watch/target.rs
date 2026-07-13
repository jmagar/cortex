use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(super) enum WatchTarget {
    Directory(PathBuf),
    File { path: PathBuf, parent: PathBuf },
}

impl WatchTarget {
    pub(super) fn root(&self) -> &Path {
        match self {
            Self::Directory(path) => path,
            Self::File { parent, .. } => parent,
        }
    }

    pub(super) fn allowed_file(&self) -> Option<&Path> {
        match self {
            Self::Directory(_) => None,
            Self::File { path, .. } => Some(path),
        }
    }
}
