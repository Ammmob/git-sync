use super::{credentials, engine, model::*, service};
use std::{fs, path::Path, process::Command};
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
fn identity(dir: &Path) {
    git(dir, &["config", "core.autocrlf", "false"]);
    git(dir, &["config", "user.name", "Sync Test"]);
    git(dir, &["config", "user.email", "sync@example.invalid"]);
}
#[test]
fn urls_and_mask() {
    assert_eq!(
        normalize_url("https://github.com/example/repo", "github").unwrap(),
        "https://github.com/example/repo.git"
    );
    assert_eq!(
        normalize_url("https://www.overleaf.com/project/123", "overleaf").unwrap(),
        "https://git@git.overleaf.com/123"
    );
    for url in [
        "https://token@github.com/a/b",
        "https://github.com/a/b?token=x",
        "https://example.com/a/b",
        "http://github.com/a/b",
        "https://git:secret@git.overleaf.com/123",
    ] {
        assert!(normalize_url(url, "github").is_err());
    }
    assert!(normalize_url("https://github.com/a/b", "overleaf").is_err());
    assert_eq!(mask("ghp_test_secret_1234"), "ghp_********1234");
    assert_eq!(mask("short"), "*****");
}
#[test]
fn config_roundtrip_and_token_guards() {
    let dir = tempfile::tempdir().unwrap();
    let service = service::Service::new(dir.path().into()).unwrap();
    service
        .request(
            "save_settings",
            serde_json::json!({"interval_seconds":17,"settle_seconds":3}),
        )
        .unwrap();
    assert_eq!(
        read_store(dir.path()).unwrap().settings.interval_seconds,
        17
    );
    assert!(service
        .request(
            "save_settings",
            serde_json::json!({"interval_seconds":0,"settle_seconds":3})
        )
        .is_err());
    assert!(credentials::token_path(dir.path(), "../escape").is_err());
    let id = "a".repeat(32);
    let mut store = service.store.lock().unwrap();
    store.catalog.tokens.push(Token {
        id: id.clone(),
        name: "test".into(),
        provider: "github".into(),
        masked: "fake".into(),
    });
    store.tasks.push(Task {
        id: "one".into(),
        name: "test".into(),
        provider: "github".into(),
        local_dir: "unused".into(),
        remote_url: "unused".into(),
        branch: "main".into(),
        interval_seconds: None,
        enabled: false,
        token_id: Some(id.clone()),
        term: SyncTerm::default(),
        expires_at: None,
    });
    drop(store);
    assert!(service
        .request("remove_token", serde_json::json!({"id":id}))
        .is_err());
    service
        .request("remove_task", serde_json::json!({"id":"one"}))
        .unwrap();
    service
        .request("remove_token", serde_json::json!({"id":id}))
        .unwrap();
    assert!(service.snapshot().catalog.tokens.is_empty());
}
#[test]
fn bidirectional_merge_conflict_and_branch_guard() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let remote = root.join("remote.git");
    let local = root.join("local");
    let peer = root.join("peer");
    let data = root.join("data");
    fs::create_dir(&data).unwrap();
    git(root, &["init", "--bare", remote.to_str().unwrap()]);
    git(
        root,
        &["clone", remote.to_str().unwrap(), local.to_str().unwrap()],
    );
    git(&local, &["checkout", "-b", "main"]);
    identity(&local);
    fs::write(local.join("paper.tex"), "base\n").unwrap();
    fs::write(local.join(".gitignore"), "*.log\n").unwrap();
    git(&local, &["add", "."]);
    git(&local, &["commit", "-m", "initial"]);
    git(&local, &["push", "origin", "main"]);
    git(
        root,
        &[
            "clone",
            "-b",
            "main",
            remote.to_str().unwrap(),
            peer.to_str().unwrap(),
        ],
    );
    identity(&peer);
    let task = Task {
        id: "test".into(),
        name: "test".into(),
        provider: "github".into(),
        local_dir: local.to_string_lossy().into(),
        remote_url: remote.to_string_lossy().into(),
        branch: "main".into(),
        interval_seconds: None,
        enabled: true,
        token_id: None,
        term: SyncTerm::default(),
        expires_at: None,
    };
    let catalog = Catalog::default();
    let cycle = || engine::cycle(&task, &catalog, &data, 0, true);
    fs::write(local.join("local.tex"), "local").unwrap();
    fs::write(local.join("ignore.log"), "ignore").unwrap();
    assert_eq!(cycle().unwrap(), "synced");
    git(&peer, &["pull", "--ff-only"]);
    assert_eq!(fs::read_to_string(peer.join("local.tex")).unwrap(), "local");
    assert!(!peer.join("ignore.log").exists());
    fs::write(peer.join("remote.tex"), "remote").unwrap();
    git(&peer, &["add", "."]);
    git(&peer, &["commit", "-m", "remote"]);
    git(&peer, &["push"]);
    cycle().unwrap();
    assert!(local.join("remote.tex").exists());
    fs::write(local.join("left.tex"), "left").unwrap();
    fs::write(peer.join("right.tex"), "right").unwrap();
    git(&peer, &["add", "."]);
    git(&peer, &["commit", "-m", "right"]);
    git(&peer, &["push"]);
    cycle().unwrap();
    git(&peer, &["pull", "--ff-only"]);
    assert!(peer.join("left.tex").exists());
    assert!(local.join("right.tex").exists());
    fs::remove_file(local.join("left.tex")).unwrap();
    cycle().unwrap();
    git(&peer, &["pull", "--ff-only"]);
    assert!(!peer.join("left.tex").exists());
    fs::write(local.join("paper.tex"), "local conflicting line\n").unwrap();
    fs::write(peer.join("paper.tex"), "remote conflicting line\n").unwrap();
    git(&peer, &["add", "."]);
    git(&peer, &["commit", "-m", "conflict"]);
    git(&peer, &["push"]);
    let remote_head = git(&peer, &["rev-parse", "HEAD"]);
    assert!(cycle().unwrap_err().blocked);
    assert!(!git(&local, &["ls-files", "-u"]).is_empty());
    assert_eq!(git(&remote, &["rev-parse", "main"]), remote_head);
    assert!(cycle().unwrap_err().blocked);
    git(&local, &["merge", "--abort"]);
    assert_eq!(
        fs::read_to_string(local.join("paper.tex")).unwrap(),
        "local conflicting line\n"
    );
    git(&local, &["checkout", "-b", "other"]);
    assert!(cycle().unwrap_err().blocked);
}
#[cfg(windows)]
#[test]
fn encrypted_token_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let service = service::Service::new(root.path().into()).unwrap();
    let token = "ghp_dummy_test_value_1234";
    service.request("add_token",serde_json::json!({"provider":"github","name":"Test","token":token,"make_default":true})).unwrap();
    let snap = service.snapshot();
    let entry = &snap.catalog.tokens[0];
    assert_eq!(entry.masked, "ghp_********1234");
    let stored = fs::read(credentials::token_path(root.path(), &entry.id).unwrap()).unwrap();
    assert!(!stored.windows(token.len()).any(|w| w == token.as_bytes()));
    let out = credentials::hidden(&mut Command::new("powershell.exe"))
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            root.path().join("helpers/askpass.ps1").to_str().unwrap(),
            "Password",
        ])
        .env(
            "GIT_SYNC_CREDENTIAL",
            credentials::token_path(root.path(), &entry.id).unwrap(),
        )
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), token);
    service
        .request("remove_token", serde_json::json!({"id":entry.id}))
        .unwrap();
    assert!(!credentials::token_path(root.path(), &entry.id)
        .unwrap()
        .exists());
}
#[test]
fn prepare_non_git_directories_preserves_local_files_and_remote_history() {
    use super::setup;
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote.git");
    let seed = root.path().join("seed");
    git(
        root.path(),
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        root.path(),
        &["clone", remote.to_str().unwrap(), seed.to_str().unwrap()],
    );
    identity(&seed);
    fs::write(seed.join("shared.txt"), "remote\n").unwrap();
    fs::write(seed.join("remote-only.txt"), "download\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-m", "seed"]);
    git(&seed, &["push", "origin", "main"]);
    let head = git(&seed, &["rev-parse", "HEAD"]);
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    for name in ["missing/nested", "empty", "nonempty"] {
        let target = root.path().join(name);
        if name != "missing/nested" {
            fs::create_dir(&target).unwrap();
        }
        if name == "nonempty" {
            fs::write(target.join("shared.txt"), "local wins\n").unwrap();
            fs::write(target.join("local-only.txt"), "keep me\n").unwrap();
        }
        let task = Task {
            id: "test".into(),
            name: "test".into(),
            provider: "github".into(),
            local_dir: target.to_string_lossy().into(),
            remote_url: remote.to_string_lossy().into(),
            branch: "main".into(),
            interval_seconds: None,
            enabled: false,
            token_id: None,
            term: SyncTerm::default(),
            expires_at: None,
        };
        assert!(setup::probe(&task.local_dir, &data).unwrap().is_none());
        if name == "nonempty" {
            assert!(setup::prepare(&task, &Catalog::default(), &data, true, false).is_err());
            assert!(!target.join(".git").exists());
        }
        setup::prepare(&task, &Catalog::default(), &data, true, true).unwrap();
        assert!(target.join(".git").is_dir());
        assert_eq!(git(&target, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            fs::read_to_string(target.join("remote-only.txt"))
                .unwrap()
                .trim(),
            "download"
        );
        if name == "nonempty" {
            assert_eq!(
                fs::read_to_string(target.join("shared.txt")).unwrap(),
                "local wins\n"
            );
            assert_eq!(
                fs::read_to_string(target.join("local-only.txt")).unwrap(),
                "keep me\n"
            );
            identity(&target);
            engine::cycle(&task, &Catalog::default(), &data, 0, true).unwrap();
            git(&seed, &["pull", "--ff-only"]);
            assert_eq!(
                fs::read_to_string(seed.join("shared.txt")).unwrap(),
                "local wins\n"
            );
            assert!(seed.join("remote-only.txt").exists());
        }
    }
}
#[test]
fn empty_remote_can_receive_first_commit_later() {
    use super::setup;
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote.git");
    git(
        root.path(),
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let target = root.path().join("new");
    let task = Task {
        id: "test".into(),
        name: "test".into(),
        provider: "github".into(),
        local_dir: target.to_string_lossy().into(),
        remote_url: remote.to_string_lossy().into(),
        branch: "main".into(),
        interval_seconds: None,
        enabled: false,
        token_id: None,
        term: SyncTerm::default(),
        expires_at: None,
    };
    setup::prepare(&task, &Catalog::default(), &data, true, true).unwrap();
    identity(&target);
    assert_eq!(
        engine::cycle(&task, &Catalog::default(), &data, 0, true).unwrap(),
        "unchanged"
    );
    fs::write(target.join("first.txt"), "first commit\n").unwrap();
    assert_eq!(
        engine::cycle(&task, &Catalog::default(), &data, 0, true).unwrap(),
        "synced"
    );
    assert_eq!(git(&remote, &["show", "main:first.txt"]), "first commit");
    assert_eq!(
        engine::cycle(&task, &Catalog::default(), &data, 0, true).unwrap(),
        "unchanged"
    );
}
#[test]
fn failed_setup_does_not_initialize_or_overwrite_destination() {
    use super::setup;
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote.git");
    let seed = root.path().join("seed");
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    git(
        root.path(),
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        root.path(),
        &["clone", remote.to_str().unwrap(), seed.to_str().unwrap()],
    );
    identity(&seed);
    fs::write(seed.join("collision"), "remote file").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-m", "seed"]);
    git(&seed, &["push", "origin", "main"]);
    let target = root.path().join("local");
    fs::create_dir_all(target.join("collision")).unwrap();
    fs::write(target.join("collision/keep.txt"), "keep").unwrap();
    let mut task = Task {
        id: "test".into(),
        name: "test".into(),
        provider: "github".into(),
        local_dir: target.to_string_lossy().into(),
        remote_url: remote.to_string_lossy().into(),
        branch: "main".into(),
        interval_seconds: None,
        enabled: false,
        token_id: None,
        term: SyncTerm::default(),
        expires_at: None,
    };
    assert!(setup::prepare(&task, &Catalog::default(), &data, true, true).is_err());
    assert!(!target.join(".git").exists());
    assert_eq!(
        fs::read_to_string(target.join("collision/keep.txt")).unwrap(),
        "keep"
    );
    task.local_dir = root.path().join("not-created").to_string_lossy().into();
    task.branch = "missing".into();
    assert!(setup::prepare(&task, &Catalog::default(), &data, true, true).is_err());
    assert!(!Path::new(&task.local_dir).exists());
    task.remote_url = root.path().join("no-remote.git").to_string_lossy().into();
    task.branch = "main".into();
    assert!(setup::prepare(&task, &Catalog::default(), &data, true, true).is_err());
    assert!(!Path::new(&task.local_dir).exists());
    assert!(setup::probe(seed.join("nested/new").to_str().unwrap(), &data).is_err());
}
#[test]
fn batch_controls_queue_all_and_preserve_running_projects() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("app");
    let workspace = root.path().join("files");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("keep.txt"), "preserve").unwrap();
    let service = service::Service::new(data.clone()).unwrap();
    {
        let mut store = service.store.lock().unwrap();
        for i in 0..6 {
            store.tasks.push(Task {
                id: i.to_string(),
                name: format!("Project {i}"),
                provider: "github".into(),
                local_dir: workspace.to_string_lossy().into(),
                remote_url: "https://github.com/example/demo.git".into(),
                branch: "main".into(),
                interval_seconds: None,
                enabled: false,
                token_id: None,
                term: SyncTerm::default(),
                expires_at: None,
            });
            store.runtime.insert(
                i.to_string(),
                Runtime {
                    blocked: i == 1,
                    ..Default::default()
                },
            );
        }
        let rt = store.runtime.get_mut("0").unwrap();
        rt.running = true;
        rt.status = "syncing".into();
    }
    let result = service
        .request(
            "batch_tasks",
            serde_json::json!({"action":"resume","all":true}),
        )
        .unwrap();
    assert_eq!(result["processed"], 6);
    let store = service.snapshot();
    assert!(store.tasks.iter().all(|t| t.enabled));
    assert_eq!(store.runtime["0"].status, "syncing");
    assert!(!store.runtime["1"].blocked);
    assert!(store
        .runtime
        .iter()
        .filter(|(id, _)| *id != "0")
        .all(|(_, rt)| rt.status == "waiting" && rt.next_run == 0 && !rt.running));
    assert!(read_store(&data).unwrap().tasks.iter().all(|t| t.enabled));
    service
        .request(
            "batch_tasks",
            serde_json::json!({"action":"pause","all":true}),
        )
        .unwrap();
    assert!(service.snapshot().tasks.iter().all(|t| !t.enabled));
    assert_eq!(service.snapshot().runtime["0"].status, "syncing");
    // A scheduler iteration that captured an ID just before pause must not start it.
    assert!(service.launch("2", false).is_err());
    let result = service
        .request(
            "batch_tasks",
            serde_json::json!({"action":"resume","ids":["2","2","missing"]}),
        )
        .unwrap();
    assert_eq!(result["processed"], 1);
    assert_eq!(result["skipped"][0]["id"], "missing");
    assert_eq!(
        service
            .snapshot()
            .tasks
            .iter()
            .filter(|t| t.enabled)
            .count(),
        1
    );
    let result = service
        .request(
            "batch_tasks",
            serde_json::json!({"action":"remove_task","ids":["0","1","2"]}),
        )
        .unwrap();
    assert_eq!(result["processed"], 2);
    assert_eq!(result["skipped"][0]["id"], "0");
    assert_eq!(service.snapshot().tasks.len(), 4);
    assert!(service.snapshot().runtime.contains_key("0"));
    assert!(!service.snapshot().runtime.contains_key("1"));
    assert_eq!(
        fs::read_to_string(workspace.join("keep.txt")).unwrap(),
        "preserve"
    );
    assert!(service
        .request(
            "batch_tasks",
            serde_json::json!({"action":"invalid","all":true})
        )
        .is_err());
    assert!(service
        .request(
            "batch_tasks",
            serde_json::json!({"action":"pause","ids":"bad"})
        )
        .is_err());
}
fn term_test_task() -> Task {
    serde_json::from_value(serde_json::json!({"id":"term", "name":"Term test", "provider":"github", "local_dir":"unused", "remote_url":"https://github.com/example/demo.git", "branch":"main", "interval_seconds":null, "enabled":true, "token_id":null})).unwrap()
}
fn local_time(year: i32, month: u8, day: u8, hour: u8) -> u64 {
    time::Date::from_calendar_date(year, time::Month::try_from(month).unwrap(), day)
        .unwrap()
        .with_hms(hour, 0, 0)
        .unwrap()
        .assume_offset(time::UtcOffset::from_hms(8, 0, 0).unwrap())
        .unix_timestamp() as u64
}
#[test]
fn term_calendar_limits_and_renewal() {
    let month = SyncTerm::month();
    assert_eq!(
        month.deadline(local_time(2027, 1, 31, 1), 28800).unwrap(),
        Some(local_time(2027, 2, 28, 1))
    );
    assert_eq!(
        month.deadline(local_time(2028, 1, 31, 1), 28800).unwrap(),
        Some(local_time(2028, 2, 29, 1))
    );
    assert_eq!(
        month.deadline(local_time(2027, 1, 1, 1), 28800).unwrap(),
        Some(local_time(2027, 2, 1, 1))
    );
    let year = SyncTerm {
        unit: "months".into(),
        count: 12,
    };
    assert_eq!(
        year.deadline(local_time(2028, 2, 29, 1), 28800).unwrap(),
        Some(local_time(2029, 2, 28, 1))
    );
    for (unit, count) in [
        ("days", 0),
        ("days", 31),
        ("months", 0),
        ("months", 13),
        ("permanent", 0),
        ("bad", 1),
    ] {
        assert!(SyncTerm {
            unit: unit.into(),
            count
        }
        .deadline(now(), 0)
        .is_err());
    }
    for count in [1, 30] {
        assert_eq!(
            SyncTerm {
                unit: "days".into(),
                count
            }
            .deadline(100, 0)
            .unwrap(),
            Some(100 + u64::from(count) * 86400)
        );
    }
    assert_eq!(SyncTerm::default().deadline(now(), 0).unwrap(), None);
    let mut old = term_test_task();
    old.term = month;
    old.update_term(None, false, 100, 0).unwrap();
    let original = old.expires_at;
    let mut edited = old.clone();
    edited.name = "Renamed".into();
    edited.expires_at = Some(9999999999);
    edited.update_term(Some(&old), false, 200, 0).unwrap();
    assert_eq!(edited.expires_at, original);
    edited.update_term(Some(&old), true, 200, 0).unwrap();
    assert_ne!(edited.expires_at, original);
    edited.term = SyncTerm {
        unit: "days".into(),
        count: 2,
    };
    edited.update_term(Some(&old), false, 300, 0).unwrap();
    assert_eq!(edited.expires_at, Some(300 + 2 * 86400));
    edited.term = SyncTerm::default();
    edited.update_term(Some(&old), false, 400, 0).unwrap();
    assert_eq!(edited.expires_at, None);
}
#[test]
fn expiry_survives_restart_and_cannot_be_bypassed_by_resume() {
    let root = tempfile::tempdir().unwrap();
    let service = service::Service::new(root.path().into()).unwrap();
    let mut task = term_test_task();
    task.term = SyncTerm {
        unit: "days".into(),
        count: 1,
    };
    task.expires_at = Some(now() - 1);
    {
        let mut s = service.store.lock().unwrap();
        s.tasks.push(task);
        let mut permanent = term_test_task();
        permanent.id = "permanent".into();
        s.tasks.push(permanent);
        s.runtime.insert(
            "term".into(),
            Runtime {
                running: true,
                status: "syncing".into(),
                blocked: true,
                ..Default::default()
            },
        );
    }
    service.expire_due().unwrap();
    assert!(!service.snapshot().tasks[0].enabled);
    assert_eq!(service.snapshot().runtime["term"].status, "syncing");
    assert!(service.launch("term", true).is_err());
    assert!(service.launch("term", false).is_err());
    assert!(service
        .request("resume", serde_json::json!({"id":"term"}))
        .is_err());
    let result = service
        .request(
            "batch_tasks",
            serde_json::json!({"action":"resume","all":true}),
        )
        .unwrap();
    assert_eq!(result["processed"], 1);
    assert_eq!(result["skipped"][0]["id"], "term");
    service
        .store
        .lock()
        .unwrap()
        .runtime
        .get_mut("term")
        .unwrap()
        .running = false;
    service.expire_due().unwrap();
    let loaded = read_store(root.path()).unwrap();
    assert_eq!(loaded.runtime["term"].status, "expired");
    assert!(loaded.runtime["term"].blocked);
    assert!(!loaded.tasks[0].enabled);
    assert!(loaded.tasks[1].enabled);
    let mut legacy = serde_json::to_value(loaded).unwrap();
    legacy["tasks"][0].as_object_mut().unwrap().remove("term");
    legacy["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("expires_at");
    legacy["tasks"][0]["enabled"] = true.into();
    fs::write(
        root.path().join("config.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let migrated = read_store(root.path()).unwrap();
    assert_eq!(migrated.tasks[0].term.unit, "permanent");
    assert!(migrated.tasks[0].enabled);
    assert_eq!(migrated.tasks[0].expires_at, None);
}
#[test]
fn save_task_assigns_default_term_and_renews_without_network() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--initial-branch", "main"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/demo.git",
        ],
    );
    let service = service::Service::new(root.path().join("data")).unwrap();
    let mut payload = serde_json::to_value(term_test_task()).unwrap();
    payload["id"] = "".into();
    payload["local_dir"] = repo.to_string_lossy().to_string().into();
    payload.as_object_mut().unwrap().remove("term");
    let id = service.request("save_task", payload).unwrap();
    let old = service.snapshot().tasks[0].clone();
    assert_eq!(old.term.unit, "months");
    assert!(old.expires_at.unwrap() > now() + 27 * 86400);
    let mut payload = serde_json::to_value(&old).unwrap();
    payload["name"] = "Renamed".into();
    payload["expires_at"] = 9999999999u64.into();
    service.request("save_task", payload).unwrap();
    assert_eq!(service.snapshot().tasks[0].expires_at, old.expires_at);
    {
        let mut s = service.store.lock().unwrap();
        s.tasks[0].expires_at = Some(now() - 10);
    }
    service.expire_due().unwrap();
    let mut payload = serde_json::to_value(service.snapshot().tasks[0].clone()).unwrap();
    payload["renew_term"] = true.into();
    payload["enabled"] = true.into();
    service.request("save_task", payload).unwrap();
    assert!(service.snapshot().tasks[0].enabled);
    assert_eq!(
        service.snapshot().runtime[id.as_str().unwrap()].status,
        "waiting"
    );
    assert!(service.snapshot().tasks[0].expires_at.unwrap() > now());
}
