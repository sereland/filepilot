use filepilot_core::{engine, oplog, rules::*, templates};
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

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("filepilot_regex_{}", uuid::Uuid::new_v4()));
        let source = root.join("downloads");
        let target = root.join("work");
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
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
        path
    }
    fn rule(&self, conditions: Vec<Condition>, actions: Vec<Action>) -> Rule {
        Rule {
            id: "quotes".into(),
            name: "报价单归档".into(),
            enabled: true,
            target_kind: TargetKind::File,
            watch_folders: vec![self.source.to_string_lossy().to_string()],
            conditions,
            actions,
        }
    }
    fn organize(&self, subfolder: &str) -> Action {
        Action::Organize {
            operation: FileOperation::Move,
            dest: self.target.to_string_lossy().into(),
            subfolder: subfolder.into(),
            date_source: DateSource::Now,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn regex(pattern: &str) -> Condition {
    Condition::NameMatch {
        operator: NameOperator::Regex,
        keyword: pattern.into(),
    }
}

fn group(mode: MatchMode, negate: bool, conditions: Vec<Condition>) -> Condition {
    Condition::Group {
        mode,
        negate,
        conditions,
    }
}

const QUOTE: &str = r"^报价_(?<客户>[^_]+)_(?P<年>\d{4})";

#[test]
fn one_rule_routes_each_file_by_extracted_values_and_undo_restores() {
    let f = Fixture::new();
    let huawei = f.file("报价_华为_2024Q3.xlsx");
    let xiaomi = f.file("报价_小米_2023.xlsx");
    let other = f.file("考勤表.xlsx");
    let rule = f.rule(
        vec![
            Condition::Extension {
                exts: vec!["xlsx".into()],
            },
            regex(QUOTE),
        ],
        vec![
            Action::Rename {
                pattern: "{年}_{客户}_{name}.{ext}".into(),
            },
            f.organize("客户/{客户}/{年}"),
        ],
    );
    rule.validate().unwrap();
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert!(plan.iter().all(|item| item.error.is_none()), "{plan:?}");
    let records = engine::apply_plan(&plan, "batch", "quotes");
    let huawei_dest = f
        .target
        .join("客户/华为/2024/2024_华为_报价_华为_2024Q3.xlsx");
    assert!(huawei_dest.exists());
    assert!(f
        .target
        .join("客户/小米/2023/2023_小米_报价_小米_2023.xlsx")
        .exists());
    assert!(other.exists() && !huawei.exists() && !xiaomi.exists());

    let log = f.root.join("oplog.jsonl");
    oplog::append_records(&log, &records).unwrap();
    oplog::undo_last_batch(&log).unwrap();
    assert!(huawei.exists() && xiaomi.exists() && !huawei_dest.exists());
}

#[test]
fn matching_ignores_case_but_captured_values_keep_original_case() {
    let f = Fixture::new();
    f.file("Quote_ACME_2024.XLSX");
    let rule = f.rule(
        vec![regex(r"^quote_(?<client>[a-z]+)_")],
        vec![f.organize("{client}")],
    );
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 1);
    assert_eq!(
        Path::new(plan[0].dest.as_ref().unwrap()),
        f.target.join("ACME").join("Quote_ACME_2024.XLSX")
    );
}

#[test]
fn regex_inside_exclusion_group_excludes_and_provides_no_variables() {
    let f = Fixture::new();
    f.file("报价_华为_2024_草稿.xlsx");
    let kept = f.file("报价_华为_2024.xlsx");
    let rule = f.rule(
        vec![
            regex(QUOTE),
            group(MatchMode::All, true, vec![regex(r"_(?<状态>草稿|作废)")]),
        ],
        vec![f.organize("{客户}")],
    );
    rule.validate().unwrap();
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].src, kept.to_string_lossy());

    let uses_excluded = f.rule(rule.conditions.clone(), vec![f.organize("{状态}")]);
    assert!(uses_excluded.validate().is_err());
}

#[test]
fn missing_capture_in_any_group_blocks_only_that_file() {
    let f = Fixture::new();
    let invoice = f.file("发票.xlsx");
    f.file("报价_华为_2024.xlsx");
    let rule = f.rule(
        vec![group(
            MatchMode::Any,
            false,
            vec![
                regex(QUOTE),
                Condition::NameMatch {
                    operator: NameOperator::Contains,
                    keyword: "发票".into(),
                },
            ],
        )],
        vec![f.organize("{客户}")],
    );
    rule.validate().unwrap();
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 2);
    let failed = plan
        .iter()
        .find(|item| item.src == invoice.to_string_lossy())
        .unwrap();
    assert!(failed.error.as_ref().unwrap().contains("客户"));
    engine::apply_plan(&plan, "batch", "quotes");
    assert!(invoice.exists());
    assert!(f.target.join("华为/报价_华为_2024.xlsx").exists());
}

#[test]
fn invalid_patterns_reserved_names_and_unknown_variables_are_rejected() {
    let f = Fixture::new();
    for pattern in ["", "  ", "(未闭合", r"(?<name>\d+)", &"a".repeat(501)] {
        assert!(
            f.rule(vec![regex(pattern)], vec![f.organize("")])
                .validate()
                .is_err(),
            "{pattern}"
        );
    }
    let unknown = f.rule(vec![regex(QUOTE)], vec![f.organize("{项目}")]);
    assert!(unknown.validate().is_err());
    let rename = f.rule(
        vec![regex(QUOTE)],
        vec![Action::Rename {
            pattern: "{客户}-{年}.{ext}".into(),
        }],
    );
    rename.validate().unwrap();
}

#[test]
fn captured_values_never_become_path_separators() {
    let meta = FileMeta {
        path: PathBuf::from("a.xlsx"),
        name: "a.xlsx".into(),
        is_directory: false,
        ext: "xlsx".into(),
        size: 1,
        created: None,
        modified: None,
        captures: [("客户".to_string(), r"..\系统".to_string())].into(),
    };
    assert!(templates::render("{客户}", &meta, DateSource::Now).is_err());
    let action = Action::Organize {
        operation: FileOperation::Move,
        dest: "D:/work".into(),
        subfolder: "{客户}".into(),
        date_source: DateSource::Now,
    };
    assert!(templates::resolve(&action, &meta).is_err());
}

#[test]
fn directory_rules_can_route_by_captured_names() {
    let f = Fixture::new();
    fs::create_dir_all(f.source.join("项目-星河-交付")).unwrap();
    let rule = Rule {
        target_kind: TargetKind::Directory,
        ..f.rule(
            vec![regex(r"^项目-(?<项目>[^-]+)-")],
            vec![f.organize("{项目}")],
        )
    };
    rule.validate().unwrap();
    std::thread::sleep(Duration::from_secs(6));
    let plan = engine::dry_run(&rule, &[f.source.clone()]);
    assert_eq!(plan.len(), 1, "{plan:?}");
    assert_eq!(
        Path::new(plan[0].dest.as_ref().unwrap()),
        f.target.join("星河").join("项目-星河-交付")
    );
}
