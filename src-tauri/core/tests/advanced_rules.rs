use chrono::{DateTime, Local};
use filepilot_core::{engine, oplog, rules::*};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
}

#[test]
fn preview_time_freezes_datetime_and_changed_metadata_or_targets_require_repreview() {
    let f = Fixture::new();
    let src = f.file("a.pdf");
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1735689600);
    let rule = f.rule(vec![
        Action::RenameWithDate {
            pattern: "{datetime}_{name}.{ext}".into(),
            date_source: DateSource::Now,
        },
        Action::Organize {
            operation: FileOperation::Move,
            dest: f.target.to_string_lossy().into(),
            subfolder: "{year}/{month}".into(),
            date_source: DateSource::Now,
        },
    ]);
    let selected = vec![src.to_string_lossy().into()];
    let preview = engine::dry_run_at(&rule, &[f.source.clone()], at);
    let same = engine::confirmed_dry_run(&rule, &[f.source.clone()], &selected, Some(&preview), at)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&same).unwrap(),
        serde_json::to_value(&preview).unwrap()
    );
    assert!(engine::confirmed_dry_run(
        &rule,
        &[f.source.clone()],
        &selected,
        Some(&preview),
        at + Duration::from_secs(5)
    )
    .is_err());
    fs::write(Path::new(preview[0].dest.as_ref().unwrap()), "occupied").unwrap();
    assert!(
        engine::confirmed_dry_run(&rule, &[f.source.clone()], &selected, Some(&preview), at)
            .is_err()
    );
    assert!(src.exists());
    let rule = f.rule(vec![Action::Organize {
        operation: FileOperation::Move,
        dest: f.target.to_string_lossy().into(),
        subfolder: "{year}/{month}/{day}".into(),
        date_source: DateSource::Modified,
    }]);
    let preview = engine::dry_run_at(&rule, &[f.source.clone()], at);
    fs::File::options()
        .write(true)
        .open(&src)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(86400 * 100))
        .unwrap();
    assert!(
        engine::confirmed_dry_run(&rule, &[f.source.clone()], &selected, Some(&preview), at)
            .is_err()
    );
    assert!(src.exists());
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("filepilot_advanced_{}", uuid::Uuid::new_v4()));
        let source = root.join("downloads");
        let target = root.join("archive");
        fs::create_dir_all(&source).unwrap();
        Self {
            root,
            source,
            target,
        }
    }
    fn file(&self, name: &str) -> PathBuf {
        let path = self.source.join(name);
        fs::write(&path, name).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(86400 * 12))
            .unwrap();
        path
    }
    fn rule(&self, actions: Vec<Action>) -> Rule {
        Rule {
            id: "invoices".into(),
            name: "发票归档".into(),
            enabled: true,
            target_kind: TargetKind::File,
            watch_folders: vec![self.source.to_string_lossy().to_string()],
            conditions: vec![Condition::Extension {
                exts: vec!["pdf".into()],
            }],
            actions,
        }
    }
    fn run(&self, rule: &Rule) -> Vec<oplog::OpRecord> {
        rule.validate().unwrap();
        let plan = engine::dry_run(rule, &[self.source.clone()]);
        engine::apply_plan(&plan, "batch", "invoices")
    }
    fn undo(&self, records: &[oplog::OpRecord]) -> std::io::Result<Vec<oplog::OpRecord>> {
        let log = self.root.join("oplog.jsonl");
        oplog::append_records(&log, records).unwrap();
        oplog::undo_last_batch(&log)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn name(operator: NameOperator, keyword: &str) -> Condition {
    Condition::NameMatch {
        operator,
        keyword: keyword.into(),
    }
}

#[test]
fn compound_invoice_conditions_use_real_jsonlogic_and_keep_legacy_rules() {
    let example: Rule =
        serde_json::from_str(include_str!("../../../tests/fixtures/invoice-rule.json")).unwrap();
    example.validate().unwrap();
    let f = Fixture::new();
    let invoice = f.file("中文发票.PDF");
    let receipt = f.file("收据.pdf");
    f.file("发票草稿.pdf");
    f.file("发票.txt");
    f.file("报告.pdf");
    let mut rule = f.rule(vec![Action::Move {
        dest: f.target.to_string_lossy().to_string(),
    }]);
    rule.conditions.push(Condition::Group {
        mode: MatchMode::Any,
        negate: false,
        conditions: vec![
            name(NameOperator::Contains, "发票"),
            name(NameOperator::Contains, "收据"),
        ],
    });
    rule.conditions
        .push(name(NameOperator::NotContains, "草稿"));
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 2);
    assert!(example.matches(&FileMeta::from_path(&invoice).unwrap()));
    assert!(!example.matches(&FileMeta::from_path(&f.source.join("发票草稿.pdf")).unwrap()));
    assert!(plan
        .iter()
        .any(|p| oplog::paths_equal(Path::new(&p.src), &invoice)));
    assert!(plan
        .iter()
        .any(|p| oplog::paths_equal(Path::new(&p.src), &receipt)));
    let old:Rule=serde_json::from_value(serde_json::json!({"id":"old","name":"旧规则","enabled":true,"watch_folders":[f.source],"conditions":[{"type":"name_contains","keyword":"中文"}],"actions":[{"type":"rename","pattern":"{name}_旧.{ext}"}]})).unwrap();
    assert!(old.matches(&FileMeta::from_path(&invoice).unwrap()));
}
#[test]
fn chinese_prefix_suffix_case_and_negated_groups_match_exactly() {
    let f = Fixture::new();
    let path = f.file("发票AbC.PDF");
    let meta = FileMeta::from_path(&path).unwrap();
    assert!(name(NameOperator::StartsWith, "发票a").matches(&meta));
    assert!(name(NameOperator::EndsWith, "c.pdf").matches(&meta));
    assert!(name(NameOperator::Equals, "发票abc.pdf").matches(&meta));
    assert!(!name(NameOperator::StartsWith, "票").matches(&meta));
    assert!(Condition::Group {
        mode: MatchMode::Any,
        negate: true,
        conditions: vec![
            name(NameOperator::Contains, "草稿"),
            name(NameOperator::Contains, "临时")
        ]
    }
    .matches(&meta));
    assert!(!Condition::Group {
        mode: MatchMode::All,
        negate: true,
        conditions: vec![]
    }
    .matches(&meta));
}
#[test]
fn dates_size_ranges_and_missing_metadata_are_fail_closed() {
    let meta = FileMeta {
        path: PathBuf::from("test.pdf"),
        name: "test.pdf".into(),
        is_directory: false,
        ext: "pdf".into(),
        size: 100,
        created: None,
        modified: Some(SystemTime::now() - Duration::from_secs(10 * 86400)),
    };
    assert!(Condition::DateMatch {
        field: FileDate::Modified,
        operator: DateOperator::OlderThanDays,
        days: 7
    }
    .matches(&meta));
    assert!(!Condition::DateMatch {
        field: FileDate::Created,
        operator: DateOperator::OlderThanDays,
        days: 7
    }
    .matches(&meta));
    assert!(!Condition::DateMatch {
        field: FileDate::Modified,
        operator: DateOperator::WithinDays,
        days: 7
    }
    .matches(&meta));
    assert!(Condition::SizeBetween { min: 100, max: 100 }.matches(&meta));
    assert!(!Condition::SizeBetween { min: 101, max: 100 }.matches(&meta));
    assert!(!Condition::CreatedWithinDays { days: u64::MAX }.matches(&meta));
    assert!(!Condition::Group {
        mode: MatchMode::All,
        negate: true,
        conditions: vec![Condition::DateMatch {
            field: FileDate::Created,
            operator: DateOperator::WithinDays,
            days: 7
        }]
    }
    .matches(&meta));
}
#[test]
fn invoice_rename_then_dynamic_move_previews_final_paths_and_undoes_as_one_file() {
    let f = Fixture::new();
    let src = f.file("发票.pdf");
    let meta = FileMeta::from_path(&src).unwrap();
    let modified: DateTime<Local> = meta.modified.unwrap().into();
    let filename = format!("{}_发票.pdf", modified.format("%Y-%m-%d"));
    let dest = f
        .target
        .join(modified.format("%Y/%m").to_string())
        .join(&filename);
    let rule = f.rule(vec![
        Action::RenameWithDate {
            pattern: "{date}_{name}.{ext}".into(),
            date_source: DateSource::Modified,
        },
        Action::Organize {
            operation: FileOperation::Move,
            dest: f.target.to_string_lossy().to_string(),
            subfolder: "{year}/{month}".into(),
            date_source: DateSource::Modified,
        },
    ]);
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].src, plan[1].src);
    assert!(oplog::paths_equal(
        Path::new(plan[1].input_src.as_deref().unwrap()),
        &f.source.join(&filename)
    ));
    assert!(oplog::paths_equal(
        Path::new(plan[1].dest.as_deref().unwrap()),
        &dest
    ));
    assert_eq!(
        engine::select_plan_sources(&plan, &[src.to_string_lossy().into()]).len(),
        2
    );
    let records = engine::apply_plan(&plan, "batch", "invoices");
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.error.is_none()));
    assert!(!src.exists());
    assert!(dest.is_file());
    assert_ne!(records[0].src, records[1].src);
    let log = f.root.join("counts.jsonl");
    oplog::append_records(&log, &records).unwrap();
    assert_eq!(oplog::rule_last_runs(&log)["invoices"].file_count, 1);
    assert_eq!(f.undo(&records).unwrap().len(), 2);
    assert_eq!(fs::read_to_string(&src).unwrap(), "发票.pdf");
    assert!(!dest.exists());
}
#[test]
fn copy_does_not_switch_working_path_and_each_copy_and_move_is_reversible() {
    let f = Fixture::new();
    let src = f.file("a.pdf");
    let backup = f.root.join("backup");
    let rule = f.rule(vec![
        Action::Copy {
            dest: backup.to_string_lossy().into(),
        },
        Action::Rename {
            pattern: "new_{name}.{ext}".into(),
        },
        Action::Move {
            dest: f.target.to_string_lossy().into(),
        },
    ]);
    let records = f.run(&rule);
    assert_eq!(records.len(), 3);
    assert!(records.iter().all(|r| r.error.is_none()));
    assert!(backup.join("a.pdf").exists());
    assert!(f.target.join("new_a.pdf").exists());
    assert_eq!(f.undo(&records).unwrap().len(), 3);
    assert!(src.exists());
    assert!(!backup.join("a.pdf").exists());
}
#[test]
fn failure_is_recorded_and_stops_only_that_files_later_actions() {
    let f = Fixture::new();
    let src = f.file("a.pdf");
    f.file("b.pdf");
    let rule = f.rule(vec![
        Action::Rename {
            pattern: "new_{name}.{ext}".into(),
        },
        Action::Move {
            dest: f.target.to_string_lossy().into(),
        },
        Action::Copy {
            dest: f.root.join("after").to_string_lossy().into(),
        },
    ]);
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    fs::create_dir_all(&f.target).unwrap();
    fs::write(f.target.join("new_a.pdf"), "occupied").unwrap();
    let records = engine::apply_plan(&plan, "batch", "invoices");
    assert_eq!(records.len(), 5);
    assert_eq!(records.iter().filter(|r| r.error.is_some()).count(), 1);
    assert!(!src.exists());
    assert!(f.source.join("new_a.pdf").exists());
    assert!(f.target.join("new_b.pdf").exists());
    assert!(!f.root.join("after/new_a.pdf").exists());
    assert_eq!(
        fs::read_to_string(f.target.join("new_a.pdf")).unwrap(),
        "occupied"
    );
}
#[test]
fn collisions_in_one_batch_are_reserved_and_selection_does_not_split_chains() {
    let f = Fixture::new();
    let a = f.file("a.pdf");
    let b = f.file("b.pdf");
    let rule = f.rule(vec![
        Action::Rename {
            pattern: "same.pdf".into(),
        },
        Action::Move {
            dest: f.target.to_string_lossy().into(),
        },
    ]);
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    let selected = engine::select_plan_sources(&plan, &[a.to_string_lossy().into()]);
    assert_eq!(selected.len(), 2);
    let records = engine::apply_plan(&selected, "batch", "invoices");
    assert!(records.iter().all(|r| r.error.is_none()));
    assert!(b.exists());
    let f2 = Fixture::new();
    f2.file("a.pdf");
    f2.file("b.pdf");
    let records = f2.run(&f2.rule(vec![
        Action::Rename {
            pattern: "same.pdf".into(),
        },
        Action::Move {
            dest: f2.target.to_string_lossy().into(),
        },
    ]));
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|r| r.error.is_none()));
    assert!(f2.target.join("same.pdf").exists());
    assert!(f2.target.join("same (1).pdf").exists());

    let f3 = Fixture::new();
    let a = f3.file("a.pdf");
    let b = f3.file("b.pdf");
    let rule = f3.rule(vec![
        Action::Rename {
            pattern: "same.pdf".into(),
        },
        Action::Move {
            dest: f3.target.to_string_lossy().into(),
        },
    ]);
    let preview = engine::dry_run(&rule, &[f3.source.clone()]);
    let selected =
        engine::selected_dry_run(&rule, &[f3.source.clone()], &[b.to_string_lossy().into()]);
    assert_eq!(
        serde_json::to_value(&selected).unwrap(),
        serde_json::to_value(engine::select_plan_sources(
            &preview,
            &[b.to_string_lossy().into()]
        ))
        .unwrap()
    );
    let records = engine::apply_plan(&selected, "batch", "invoices");
    assert!(records.iter().all(|r| r.error.is_none()));
    assert!(a.exists());
    assert!(f3.target.join("same (1).pdf").exists());
}
#[test]
fn changed_files_and_occupied_intermediate_paths_block_undo_before_any_restore() {
    let f = Fixture::new();
    let src = f.file("a.pdf");
    let rule = f.rule(vec![
        Action::Rename {
            pattern: "renamed.pdf".into(),
        },
        Action::Move {
            dest: f.target.to_string_lossy().into(),
        },
    ]);
    let records = f.run(&rule);
    fs::write(f.source.join("renamed.pdf"), "unrelated").unwrap();
    assert!(f.undo(&records).is_err());
    assert!(f.target.join("renamed.pdf").exists());
    assert!(!src.exists());
    assert_eq!(
        fs::read_to_string(f.source.join("renamed.pdf")).unwrap(),
        "unrelated"
    );
    fs::remove_file(f.source.join("renamed.pdf")).unwrap();
    fs::write(f.target.join("renamed.pdf"), "modified").unwrap();
    let log = f.root.join("another.jsonl");
    oplog::append_records(&log, &records).unwrap();
    assert!(oplog::undo_last_batch(&log).is_err());
    assert!(!src.exists());
    assert!(oplog::read_all(&log).iter().all(|r| !r.undone));
}
#[test]
fn traversal_invalid_templates_missing_dates_and_changed_preview_never_execute() {
    let f = Fixture::new();
    let src = f.file("a.pdf");
    for subfolder in ["../outside", "C:/outside", "/outside", "{unknown}", "CON"] {
        let rule = f.rule(vec![Action::Organize {
            operation: FileOperation::Move,
            dest: f.target.to_string_lossy().into(),
            subfolder: subfolder.into(),
            date_source: DateSource::Modified,
        }]);
        let plan = engine::dry_run(&rule, &[f.source.clone()]);
        assert!(plan[0].error.is_some(), "{subfolder}");
        let records = engine::apply_plan(&plan, "batch", "invoices");
        assert!(records[0].error.is_some());
        assert!(src.exists());
    }
    let rule = f.rule(vec![
        Action::Rename {
            pattern: "new_{name}.{ext}".into(),
        },
        Action::Move {
            dest: f.target.to_string_lossy().into(),
        },
    ]);
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    fs::write(&src, "changed").unwrap();
    let records = engine::apply_plan(&plan, "batch", "invoices");
    assert_eq!(records.len(), 1);
    assert!(records[0].error.is_some());
    assert!(src.exists());
    let mut meta = FileMeta::from_path(&src).unwrap();
    meta.created = None;
    assert!(filepilot_core::templates::render("{year}", &meta, DateSource::Created).is_err());
}
#[test]
fn recycle_cannot_be_followed_by_another_action_and_groups_are_validated_recursively() {
    let f = Fixture::new();
    let rule = f.rule(vec![
        Action::MoveToRecycleBin,
        Action::Move {
            dest: f.target.to_string_lossy().into(),
        },
    ]);
    assert!(rule.validate().is_err());
    let mut rule = f.rule(vec![Action::MoveToRecycleBin]);
    rule.target_kind = TargetKind::Directory;
    rule.conditions = vec![Condition::Group {
        mode: MatchMode::All,
        negate: false,
        conditions: vec![Condition::Extension {
            exts: vec!["zip".into()],
        }],
    }];
    assert!(rule.validate().is_err());
    if let Condition::Group { negate, .. } = &mut rule.conditions[0] {
        *negate = true;
    }
    assert!(!rule.matches(&FileMeta::from_path(&f.source).unwrap()));
}

#[test]
fn automatic_rename_and_copy_protect_unchanged_files_until_modified() {
    let f = Fixture::new();
    let src = f.file("a.pdf");
    let backup = f.root.join("backup");
    let rule = f.rule(vec![
        Action::Copy {
            dest: backup.to_string_lossy().into(),
        },
        Action::Rename {
            pattern: "new_{name}.{ext}".into(),
        },
    ]);
    let records = f.run(&rule);
    let mut protected = Vec::new();
    filepilot_core::protection::protect_organized(&records, &mut protected);
    let skip = |path: &Path| {
        protected
            .iter()
            .any(|p| oplog::paths_equal(&p.path, path) && filepilot_core::protection::unchanged(p))
    };
    assert!(skip(&f.source.join("new_a.pdf")));
    assert!(engine::dry_run_filtered(&rule, &[f.source.clone()], &|p| !skip(p)).is_empty());
    assert!(!src.exists());
    fs::write(f.source.join("new_a.pdf"), "changed contents").unwrap();
    assert!(!skip(&f.source.join("new_a.pdf")));
    let f = Fixture::new();
    let src = f.file("a.pdf");
    let records = f.run(&f.rule(vec![Action::Copy {
        dest: f.target.to_string_lossy().into(),
    }]));
    let mut protected = Vec::new();
    filepilot_core::protection::protect_organized(&records, &mut protected);
    assert!(protected.iter().any(|p| oplog::paths_equal(&p.path, &src)));
}
