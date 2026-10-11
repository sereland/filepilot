use filepilot_core::{
    directory, engine, oplog,
    rules::{Action, Condition, FileMeta, Rule, TargetKind},
    watcher,
};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
    log: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("filepilot-directory-{}", uuid::Uuid::new_v4()));
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        let log = root.join("oplog.jsonl");
        Self {
            root,
            source,
            target,
            log,
        }
    }
    fn folder(&self, name: &str) -> PathBuf {
        let path = self.source.join(name);
        fs::create_dir_all(path.join("nested/empty")).unwrap();
        fs::write(path.join("nested/app.exe"), "original").unwrap();
        fs::write(path.join(".config"), "hidden content").unwrap();
        make_old(&path);
        path
    }
    fn rule(&self, action: Action) -> Rule {
        Rule {
            id: "directory-rule".into(),
            name: "项目文件夹".into(),
            enabled: true,
            target_kind: TargetKind::Directory,
            watch_folders: vec![self.source.to_string_lossy().into()],
            conditions: vec![Condition::NameContains {
                keyword: "project".into(),
            }],
            actions: vec![action],
        }
    }
    fn execute(&self, rule: &Rule) -> Vec<oplog::OpRecord> {
        let plan = engine::dry_run(rule, &[self.source.clone()]);
        assert_eq!(plan.len(), 1);
        assert!(plan[0].is_directory);
        let records = engine::apply_plan(&plan, "batch", &rule.id);
        assert_eq!(records.len(), 1);
        assert!(records[0].error.is_none(), "{:?}", records[0].error);
        oplog::append_records(&self.log, &records).unwrap();
        records
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn make_old(path: &Path) {
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            make_old(&entry.unwrap().path());
        }
    }
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(0x100).custom_flags(0x02000000);
    }
    options
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(20))
        .unwrap();
}

#[test]
fn legacy_rules_default_to_files_and_directory_rules_roundtrip() {
    let legacy: Rule = serde_json::from_str(r#"{"id":"old","name":"old","enabled":true,"watch_folders":[],"conditions":[],"actions":[]}"#).unwrap();
    assert_eq!(legacy.target_kind, TargetKind::File);
    let f = Fixture::new();
    let rule = f.rule(Action::MoveToRecycleBin);
    let loaded: Rule = serde_json::from_str(&serde_json::to_string(&rule).unwrap()).unwrap();
    assert_eq!(loaded.target_kind, TargetKind::Directory);
}

#[test]
fn directory_rules_scan_only_top_level_and_do_not_match_files_or_extensions() {
    let f = Fixture::new();
    let folder = f.folder("project.v1");
    fs::create_dir_all(folder.join("project.inner")).unwrap();
    fs::write(f.source.join("project.v1.zip"), "file").unwrap();
    make_old(&f.source);
    let rule = f.rule(Action::MoveToRecycleBin);
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 1);
    assert!(oplog::paths_equal(Path::new(&plan[0].src), &folder));
    let meta = FileMeta::from_path(&folder).unwrap();
    assert!(meta.is_directory);
    assert!(meta.ext.is_empty());
    assert!(!Condition::Extension {
        exts: vec!["v1".into()]
    }
    .matches(&meta));
    assert!(!Condition::SizeGreaterThan { bytes: 1 }.matches(&meta));
    assert_eq!(engine::scan_folder(&f.source).len(), 1);
}

#[test]
fn nested_writes_and_unfinished_downloads_block_directory_processing() {
    let f = Fixture::new();
    let folder = f.folder("project");
    let rule = f.rule(Action::MoveToRecycleBin);
    fs::write(folder.join("nested/app.exe"), "still writing").unwrap();
    assert!(engine::dry_run(&rule, &[f.source.clone()]).is_empty());
    make_old(&folder);
    assert_eq!(engine::dry_run(&rule, &[f.source.clone()]).len(), 1);
    fs::write(folder.join("nested/app.zip.crdownload"), "partial").unwrap();
    make_old(&folder);
    assert!(engine::dry_run(&rule, &[f.source.clone()]).is_empty());
    assert!(!directory::settled(&folder, Duration::from_secs(5)));
}

#[test]
fn directory_move_preserves_dotted_names_collisions_and_occupied_undo_source() {
    let f = Fixture::new();
    let source = f.folder("project.v1");
    let existing = f.target.join("project.v1");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("keep.txt"), "keep").unwrap();
    let rule = f.rule(Action::Move {
        dest: f.target.to_string_lossy().into(),
    });
    let records = f.execute(&rule);
    let destination = f.target.join("project.v1 (1)");
    assert!(oplog::paths_equal(
        Path::new(records[0].dest.as_ref().unwrap()),
        &destination
    ));
    assert!(!source.exists());
    assert_eq!(
        fs::read_to_string(destination.join("nested/app.exe")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(existing.join("keep.txt")).unwrap(),
        "keep"
    );
    let run = &oplog::rule_last_runs(&f.log)[&rule.id];
    assert_eq!((run.file_count, run.directory_count), (1, 1));
    fs::create_dir(&source).unwrap();
    fs::write(source.join("occupied.txt"), "untouched").unwrap();
    let undone = oplog::undo_last_batch(&f.log).unwrap();
    let restored = f.source.join("project.v1 (1)");
    assert!(restored.join("nested/empty").is_dir());
    assert!(oplog::paths_equal(
        Path::new(&undone[0].restored_sources[0]),
        &restored
    ));
    assert_eq!(
        fs::read_to_string(source.join("occupied.txt")).unwrap(),
        "untouched"
    );
    assert!(!destination.exists());
}

#[test]
fn directory_copy_preserves_tree_and_refuses_undo_of_modified_content() {
    let f = Fixture::new();
    let source = f.folder("project.v1");
    let rule = f.rule(Action::Copy {
        dest: f.target.to_string_lossy().into(),
    });
    f.execute(&rule);
    let destination = f.target.join("project.v1");
    assert!(source.exists());
    assert!(destination.join("nested/empty").is_dir());
    assert_eq!(
        fs::read_to_string(destination.join(".config")).unwrap(),
        "hidden content"
    );
    fs::write(destination.join("nested/app.exe"), "user edits").unwrap();
    assert!(oplog::undo_last_batch(&f.log).is_err());
    assert_eq!(
        fs::read_to_string(destination.join("nested/app.exe")).unwrap(),
        "user edits"
    );
    assert!(!oplog::read_all(&f.log)[0].undone);
    fs::write(destination.join("nested/app.exe"), "original").unwrap();
    assert_eq!(oplog::undo_last_batch(&f.log).unwrap().len(), 1);
    assert!(!destination.exists());
    assert_eq!(
        fs::read_to_string(source.join("nested/app.exe")).unwrap(),
        "original"
    );
}

#[test]
fn directory_recycle_restores_the_entire_tree() {
    let f = Fixture::new();
    let source = f.folder("project.v1");
    let rule = f.rule(Action::MoveToRecycleBin);
    f.execute(&rule);
    assert!(!source.exists());
    assert_eq!(oplog::undo_last_batch(&f.log).unwrap().len(), 1);
    assert!(source.join("nested/empty").is_dir());
    assert_eq!(
        fs::read_to_string(source.join("nested/app.exe")).unwrap(),
        "original"
    );
    assert!(oplog::undo_last_batch(&f.log).unwrap().is_empty());
}

#[test]
fn protection_reads_legacy_files_and_protects_empty_directories() {
    let f = Fixture::new();
    let file = f.source.join("a.txt");
    fs::write(&file, "data").unwrap();
    let protected = filepilot_core::protection::protection(&file).unwrap();
    let mut json = serde_json::to_value(&protected).unwrap();
    json.as_object_mut().unwrap().remove("directory_state");
    let loaded: filepilot_core::protection::ProtectedFile = serde_json::from_value(json).unwrap();
    assert!(filepilot_core::protection::unchanged(&loaded));
    let empty = f.source.join("empty");
    fs::create_dir(&empty).unwrap();
    let mut entries = Vec::new();
    filepilot_core::protection::protect_restored(&empty, &mut entries);
    filepilot_core::protection::protect_restored(&empty, &mut entries);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].directory_state.is_some());
    assert!(filepilot_core::protection::unchanged(&entries[0]));
    fs::write(empty.join("new.txt"), "new").unwrap();
    assert!(!filepilot_core::protection::unchanged(&entries[0]));
}

#[test]
#[cfg(windows)]
fn directory_copy_preserves_readonly_files_and_download_origin_streams() {
    let f = Fixture::new();
    let source = f.folder("project");
    let file = source.join("nested/app.exe");
    let stream = format!("{}:Zone.Identifier", file.display());
    fs::write(&stream, "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();
    make_old(&source);
    let destination = f.target.join("project");
    directory::copy_to(
        &source,
        &destination,
        &filepilot_core::archive::fingerprint(&source).unwrap(),
    )
    .unwrap();
    let copied = destination.join("nested/app.exe");
    assert!(fs::metadata(&copied).unwrap().permissions().readonly());
    assert_eq!(
        fs::read_to_string(format!("{}:Zone.Identifier", copied.display())).unwrap(),
        "[ZoneTransfer]\r\nZoneId=3\r\n"
    );
    let occupied = f.target.join("occupied");
    fs::create_dir(&occupied).unwrap();
    assert!(directory::copy_to(
        &source,
        &occupied,
        &filepilot_core::archive::fingerprint(&source).unwrap()
    )
    .is_err());
    assert_eq!(
        fs::read_dir(&f.target).unwrap().count(),
        2,
        "复制失败应清理含只读文件的临时目录"
    );
    for path in [&file, &copied] {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
}

#[test]
fn directory_rename_uses_full_name_and_rejects_path_traversal() {
    let f = Fixture::new();
    let source = f.folder("project.v1");
    let rule = f.rule(Action::Rename {
        pattern: "{name}_archived".into(),
    });
    let record = &f.execute(&rule)[0];
    assert_eq!(
        Path::new(record.dest.as_ref().unwrap())
            .file_name()
            .unwrap(),
        "project.v1_archived"
    );
    oplog::undo_last_batch(&f.log).unwrap();
    make_old(&source);
    let invalid = f.rule(Action::Rename {
        pattern: "../outside".into(),
    });
    let plan = engine::dry_run(&invalid, &[f.source.clone()]);
    assert!(plan[0].error.is_some());
    let records = engine::apply_plan(&plan, "bad", &invalid.id);
    assert!(records[0].error.is_some());
    assert!(source.join("nested/app.exe").exists());
    assert!(!f.root.join("outside").exists());
}

#[test]
fn changed_preview_and_occupied_destination_leave_source_untouched() {
    let f = Fixture::new();
    let source = f.folder("project");
    let rule = f.rule(Action::Move {
        dest: f.target.to_string_lossy().into(),
    });
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    fs::write(source.join("nested/app.exe"), "changed").unwrap();
    make_old(&source);
    let records = engine::apply_plan(&plan, "changed", &rule.id);
    assert!(records[0].error.is_some());
    assert!(source.exists());
    assert!(!f.target.join("project").exists());
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    fs::create_dir(f.target.join("project")).unwrap();
    fs::write(f.target.join("project/keep"), "keep").unwrap();
    let records = engine::apply_plan(&plan, "occupied", &rule.id);
    assert!(records[0].error.is_some());
    assert!(source.exists());
    assert_eq!(
        fs::read_to_string(f.target.join("project/keep")).unwrap(),
        "keep"
    );
}

#[test]
fn copy_failure_cleans_staging_and_never_merges_or_overwrites() {
    let f = Fixture::new();
    let source = f.folder("project");
    let destination = f.target.join("project");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep"), "keep").unwrap();
    let digest = filepilot_core::archive::fingerprint(&source).unwrap();
    assert!(directory::copy_to(&source, &destination, &digest).is_err());
    assert_eq!(
        fs::read_to_string(destination.join("keep")).unwrap(),
        "keep"
    );
    assert!(source.join("nested/app.exe").exists());
    assert_eq!(fs::read_dir(&f.target).unwrap().count(), 1);
    assert!(directory::copy_to(&source, &source.join("nested/copy"), &digest).is_err());
    assert!(!source.join("nested/copy").exists());
}

#[test]
fn directory_watch_protection_detects_nested_changes() {
    let f = Fixture::new();
    let source = f.folder("project");
    let rule = f.rule(Action::Copy {
        dest: f.target.to_string_lossy().into(),
    });
    let mut protected = Vec::new();
    filepilot_core::protection::protect_restored(&source, &mut protected);
    assert_eq!(protected.len(), 1);
    let protected: Vec<filepilot_core::protection::ProtectedFile> =
        serde_json::from_str(&serde_json::to_string(&protected).unwrap()).unwrap();
    let should_skip = |path: &Path| {
        protected.iter().any(|entry| {
            oplog::paths_equal(path, &entry.path) && filepilot_core::protection::unchanged(entry)
        })
    };
    assert!(watcher::apply_rules_for_folder(&[rule.clone()], &f.source, &should_skip).is_empty());
    let root_modified = fs::metadata(&source).unwrap().modified().unwrap();
    fs::write(source.join("nested/app.exe"), "changed internally").unwrap();
    make_old(&source.join("nested/app.exe"));
    assert_eq!(
        fs::metadata(&source).unwrap().modified().unwrap(),
        root_modified
    );
    assert!(!filepilot_core::protection::unchanged(&protected[0]));
    let results = watcher::apply_rules_for_folder(&[rule], &f.source, &should_skip);
    assert_eq!(results.len(), 1);
    assert!(results[0].1[0].error.is_none());
    assert_eq!(
        fs::read_to_string(f.target.join("project/nested/app.exe")).unwrap(),
        "changed internally"
    );
}

#[test]
fn recursive_watch_notices_writes_inside_a_directory() {
    let f = Fixture::new();
    let source = f.folder("project");
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = watcher::start_watching_with_modes(
        vec![(f.source.clone(), true)],
        Box::new(move |path| {
            let _ = tx.send(path.to_path_buf());
            0
        }),
    );
    fs::write(source.join("nested/app.exe"), "nested change").unwrap();
    let event = rx.recv_timeout(Duration::from_secs(15));
    handle.shutdown();
    assert!(oplog::paths_equal(
        &event.expect("子目录写入必须触发监控"),
        &f.source
    ));
}

#[test]
fn multiple_directory_actions_are_blocked_and_output_container_is_excluded() {
    let f = Fixture::new();
    let source = f.folder("project");
    let destination = f.source.join("project-output");
    fs::create_dir(&destination).unwrap();
    make_old(&destination);
    let mut rule = f.rule(Action::Copy {
        dest: destination.to_string_lossy().into(),
    });
    rule.actions.push(Action::MoveToRecycleBin);
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 1);
    assert!(plan[0].error.is_some());
    assert!(engine::apply_plan(&plan, "multiple", &rule.id)[0]
        .error
        .is_some());
    assert!(source.join("nested/app.exe").exists());
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
}

#[test]
fn directory_selection_keeps_excluded_and_new_folders_untouched() {
    let f = Fixture::new();
    let chosen = f.folder("project-chosen");
    let excluded = f.folder("project-excluded");
    let rule = f.rule(Action::Move {
        dest: f.target.to_string_lossy().into(),
    });
    let preview = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(preview.len(), 2);
    let added = f.folder("project-new");
    let selected = engine::select_plan_sources(
        &engine::dry_run(&rule, &[f.source.clone()]),
        &[chosen.to_string_lossy().into()],
    );
    let records = engine::apply_plan(&selected, "chosen", &rule.id);
    assert_eq!(records.len(), 1);
    assert!(records[0].error.is_none());
    assert!(!chosen.exists());
    assert!(excluded.join("nested/app.exe").exists());
    assert!(added.join("nested/app.exe").exists());
}

#[test]
#[cfg(windows)]
fn locked_directory_move_preserves_source_and_does_not_publish_target() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let source = f.folder("project");
    let rule = f.rule(Action::Move {
        dest: f.target.to_string_lossy().into(),
    });
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    let _locked = File::options()
        .read(true)
        .share_mode(0)
        .open(source.join("nested/app.exe"))
        .unwrap();
    let records = engine::apply_plan(&plan, "locked", &rule.id);
    assert!(records[0].error.is_some());
    assert!(source.exists());
    assert!(!f.target.join("project").exists());
}

#[test]
#[cfg(windows)]
fn directory_junctions_are_not_followed() {
    let f = Fixture::new();
    let outside = f.target.join("actual");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "keep").unwrap();
    let junction = f.source.join("project-junction");
    let result = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result);
    let rule = f.rule(Action::MoveToRecycleBin);
    assert!(engine::dry_run(&rule, &[f.source.clone()]).is_empty());
    assert!(filepilot_core::archive::fingerprint(&junction).is_err());
    fs::remove_dir(&junction).unwrap();
    assert_eq!(fs::read_to_string(outside.join("keep")).unwrap(), "keep");
}

#[test]
#[cfg(windows)]
fn cross_volume_copy_works_and_move_failure_preserves_source() {
    let f = Fixture::new();
    let source = f.folder("project");
    let target = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("filepilot-cross-volume-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&target).unwrap();
    let cleanup = Fixture {
        root: target.clone(),
        source: target.clone(),
        target: target.clone(),
        log: target.join("unused"),
    };
    let destination = target.join("project");
    let digest = filepilot_core::archive::fingerprint(&source).unwrap();
    directory::copy_to(&source, &destination, &digest).unwrap();
    assert_eq!(
        filepilot_core::archive::fingerprint(&destination).unwrap(),
        digest
    );
    let src_prefix = fs::canonicalize(&source)
        .unwrap()
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_owned();
    let dest_prefix = fs::canonicalize(&target)
        .unwrap()
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_owned();
    if src_prefix != dest_prefix {
        assert!(directory::move_to(&source, &target.join("moved"), &digest).is_err());
        assert!(source.join("nested/app.exe").exists());
        assert!(!target.join("moved").exists());
    }
    drop(cleanup);
}
