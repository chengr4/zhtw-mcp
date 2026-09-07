use super::*;

#[cfg(unix)]
#[test]
fn an_existing_mode_survives_the_replacement() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("kept.json");
    std::fs::write(&dest, b"old").unwrap();
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o644)).unwrap();

    replace_file(&dest, b"new").unwrap();

    let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o644,
        "replacing a file must not change who can read it"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"new");
}

#[cfg(unix)]
#[test]
fn a_new_file_stays_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("fresh.json");
    replace_file(&dest, b"new").unwrap();

    let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "a file this code creates is the user's alone");
}

#[test]
fn replaces_existing_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.json");
    std::fs::write(&path, b"old").unwrap();
    replace_file(&path, b"new").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
}

#[test]
fn creates_missing_parent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/deeper/f.json");
    replace_file(&path, b"x").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"x");
}

#[test]
fn leaves_no_temp_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.json");
    replace_file(&path, b"x").unwrap();
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries.len(), 1, "temp file should be renamed, not left");
}

#[test]
fn concurrent_writers_do_not_share_a_temp_path() {
    // The whole point of a unique temp name: two writers targeting one
    // destination must not collide, and the loser must not truncate.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.json");
    std::thread::scope(|s| {
        for i in 0..8 {
            let path = path.clone();
            s.spawn(move || {
                let payload = vec![b'a' + i as u8; 4096];
                replace_file(&path, &payload).unwrap();
            });
        }
    });

    // Whoever won, the file is one writer's payload in full, never a mix or a
    // truncation.
    let got = std::fs::read(&path).unwrap();
    assert_eq!(got.len(), 4096);
    assert!(got.iter().all(|&b| b == got[0]), "torn write");
}
