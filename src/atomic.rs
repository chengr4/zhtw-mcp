// Atomic file replacement.
//
// One implementation on purpose. This existed three times: the override store,
// the scan cache, and the judgment cache. The copy that drifted wrote to a
// fixed .tmp path, so two processes sharing the destination raced on it, and
// fell back to writing straight over the live file when the rename failed,
// which is the exact truncation the design exists to prevent.

use std::io::Write;
use std::path::Path;

/// Replace `dest` with `bytes`, atomically.
///
/// The temp file is created in the destination's own directory so the
/// rename stays within one filesystem, and it carries a unique name so
/// two processes writing the same target cannot collide on it.  Missing
/// parent directories are created.
///
/// Note that rename semantics detach a symlinked destination and ignore
/// the umask in favor of the temp file's mode; callers that care about
/// either (`baseline.rs` does) should write in place instead. An existing
/// destination's own mode is carried across, so a replacement is not a
/// permission change.
pub fn replace_file(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    preserve_mode(dest, &tmp)?;
    persist_with_retry(tmp, dest)
}

/// Carry an existing destination's permission bits onto the replacement.
///
/// The rename takes the temp file's mode with it and NamedTempFile creates at
/// 0600, so without this, replacing a file somebody had deliberately made
/// group-readable would narrow it and say nothing. A destination that does not
/// exist keeps the 0600: a new cache or override file belongs to the user who
/// ran this, and inheriting the umask would be a wider default than the one
/// this code would choose.
#[cfg(unix)]
fn preserve_mode(dest: &Path, tmp: &tempfile::NamedTempFile) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // Through the link rather than at it: what is being replaced is the file a
    // reader would open.
    let Ok(meta) = std::fs::metadata(dest) else {
        return Ok(());
    };
    let mode = meta.permissions().mode() & 0o7777;
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode))
}

/// Not implemented off unix, and the gap is real rather than absent: a Windows
/// replacement keeps the temp file's own security descriptor, so an explicit
/// ACL somebody set on the destination does not survive. What saves it in
/// practice is that these files live under the user's own cache and config
/// directories, where the descriptor is inherited from the parent and the
/// temp file inherits the same one. Copying a descriptor properly needs a
/// Windows API this crate does not otherwise link, which is a large dependency
/// for a case nothing has reported.
#[cfg(not(unix))]
fn preserve_mode(_dest: &Path, _tmp: &tempfile::NamedTempFile) -> std::io::Result<()> {
    Ok(())
}

/// Persist `tmp` to `dest`, retrying a transient Windows failure.
///
/// `NamedTempFile::persist` on Windows is one `MoveFileExW` call with no
/// retry of its own, and replacing a file that way is not race-free the way
/// POSIX `rename(2)` is: two callers racing the same `dest` can make one
/// call's `MoveFileExW` fail with `ERROR_ACCESS_DENIED` (surfaced here as
/// `PermissionDenied`) even though neither caller did anything wrong. A short
/// bounded retry clears it; `uv` hit the identical failure persisting temp
/// files and fixed it the same way (astral-sh/uv#9543).
fn persist_with_retry(mut tmp: tempfile::NamedTempFile, dest: &Path) -> std::io::Result<()> {
    const MAX_ATTEMPTS: u32 = 6;
    let mut delay_ms = 10u64;
    let mut attempt = 1;
    loop {
        match tmp.persist(dest) {
            Ok(_) => return Ok(()),
            Err(err) => {
                let retryable = cfg!(windows)
                    && err.error.kind() == std::io::ErrorKind::PermissionDenied
                    && attempt < MAX_ATTEMPTS;
                if !retryable {
                    return Err(err.error);
                }
                tmp = err.file;
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                delay_ms *= 2;
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/atomic/tests.rs"]
mod tests;
