//! Temporary files (M31, spec §117): the status lists ChosProxy's
//! temporary directory, "Clean now" removes only this run's unused
//! leftovers, old leftovers of other runs go only on request, and nothing
//! else — the open project, files ChosProxy did not make, links and what
//! they point to — is ever removed.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, TempKind};
use chosproxy_core::paths::AppPaths;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn write(path: &Path, bytes: usize) {
    std::fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn age(path: &Path, by: Duration) {
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - by).unwrap();
}

#[tokio::test]
async fn cleanup_removes_only_safe_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(dir.path());
    let engine = Engine::new(paths.clone()).unwrap();
    let open = engine.new_temporary_project().await.unwrap();
    let tmp = &paths.temp_dir;
    let me = std::process::id();
    let other = me.wrapping_add(1);

    // This run's leftover (a temporary project that was not deleted).
    let mine = tmp.join(format!("temp-{me}-1-9.chosproxy"));
    write(&mine, 1000);
    let mine_profile = tmp.join(format!("browser-{me}-5"));
    std::fs::create_dir(&mine_profile).unwrap();
    write(&mine_profile.join("Cookies"), 500);
    // Another run's files: one recent, one old.
    let recent = tmp.join(format!("temp-{other}-1-0.chosproxy"));
    write(&recent, 300);
    let old = tmp.join(format!("temp-{other}-2-0.chosproxy"));
    write(&old, 200);
    age(&old, Duration::from_secs(3 * 24 * 3600));
    // Not ChosProxy's, even with a similar name or old.
    let user = tmp.join("notes.txt");
    write(&user, 50);
    age(&user, Duration::from_secs(30 * 24 * 3600));
    let lookalike = tmp.join("temp-notes.chosproxy.bak");
    write(&lookalike, 50);
    // A link with one of our names pointing outside: kept, and so is its
    // target.
    let outside = dir.path().join("precious.chosproxy");
    write(&outside, 70);
    #[cfg(unix)]
    let link = {
        let l = tmp.join(format!("temp-{me}-3-0.chosproxy"));
        std::os::unix::fs::symlink(&outside, &l).unwrap();
        l
    };

    let status = engine.temp_status().await.unwrap();
    let find = |name: &str| {
        status
            .entries
            .iter()
            .find(|e| e.name == name)
            .unwrap()
            .clone()
    };
    let open_name = Path::new(&open.path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let current = find(&open_name);
    assert!(current.in_use && !current.cleanable && !current.removable);
    assert!(find(&mine.file_name().unwrap().to_string_lossy()).cleanable);
    let profile = find(&mine_profile.file_name().unwrap().to_string_lossy());
    assert_eq!(profile.kind, TempKind::BrowserProfile);
    assert_eq!(profile.bytes, 500);
    let r = find(&recent.file_name().unwrap().to_string_lossy());
    assert!(!r.cleanable && !r.removable && !r.this_run);
    let o = find(&old.file_name().unwrap().to_string_lossy());
    assert!(!o.cleanable && o.removable);
    assert_eq!(find("notes.txt").kind, TempKind::Other);
    assert!(!find("notes.txt").removable);
    assert_eq!(status.cleanable_bytes, 1500);

    // Clean now: only this run's unused leftovers.
    let done = engine.temp_cleanup().await.unwrap();
    assert_eq!(done.removed, 2, "{done:?}");
    assert!(!mine.exists() && !mine_profile.exists());
    for kept in [&recent, &old, &user, &lookalike, &outside] {
        assert!(kept.exists(), "{} was removed", kept.display());
    }
    assert!(Path::new(&open.path).exists(), "the open project stays");
    #[cfg(unix)]
    assert!(std::fs::symlink_metadata(&link).is_ok(), "links are kept");

    // On request: only old leftovers of other runs; the rest is refused.
    let names = vec![
        old.file_name().unwrap().to_string_lossy().into_owned(),
        recent.file_name().unwrap().to_string_lossy().into_owned(),
        "notes.txt".to_string(),
        open_name.clone(),
        "../precious.chosproxy".to_string(),
    ];
    let done = engine.temp_remove(names).await.unwrap();
    assert_eq!(done.removed, 1);
    assert_eq!(done.failed.len(), 4, "{done:?}");
    assert!(!old.exists());
    for kept in [&recent, &user, &outside] {
        assert!(kept.exists(), "{} was removed", kept.display());
    }
    assert!(Path::new(&open.path).exists());

    // Shutting down deletes this run's temporary project and nothing else.
    engine.shutdown().await;
    assert!(!Path::new(&open.path).exists());
    for kept in [&recent, &user, &lookalike, &outside] {
        assert!(kept.exists(), "{} was removed at exit", kept.display());
    }
}
