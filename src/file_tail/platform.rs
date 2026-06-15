//! Platform-specific primitives for file-tail ingest.
//!
//! The file-tail security and rotation model is Unix-centric: it uses the
//! `(st_dev, st_ino)` pair to detect log rotation/truncation and opens files
//! with `O_NOFOLLOW` to defeat symlink-swap TOCTOU attacks. Those concepts have
//! no direct Windows equivalent, so this module isolates the two
//! platform-specific operations behind a stable, cross-platform surface.
//!
//! On Unix the behavior is exactly as before. On Windows file identity is not
//! cheaply available for path-based metadata, so [`metadata_identity`] returns
//! a constant and rotation detection falls back to the content fingerprint and
//! file length (see `supervisor::reopen_if_rotated_or_truncated`). The
//! no-follow open maps to `FILE_FLAG_OPEN_REPARSE_POINT`, which mirrors the
//! intent of `O_NOFOLLOW` (open the reparse point itself rather than its
//! target).

use std::fs::{File, Metadata, OpenOptions};
use std::path::Path;

/// File identity used to detect rotation/truncation.
///
/// Unix: `(st_dev, st_ino)`. Windows: `(0, 0)` — identity-based rotation
/// detection is disabled and the caller relies on the content fingerprint and
/// length instead.
pub(crate) fn metadata_identity(metadata: &Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(windows)]
    {
        let _ = metadata;
        (0, 0)
    }
}

/// Open `path` read-only without following a terminal symlink / reparse point.
pub(crate) fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT — open the reparse point itself rather
        // than following it, mirroring O_NOFOLLOW's symlink-swap protection.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}
