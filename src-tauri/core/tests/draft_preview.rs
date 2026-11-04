use filepilot_core::{engine, rules::*};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime},
};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("filepilot_draft_{}", uuid::Uuid::new_v4()));
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
            id: String::new(),
            name: String::new(),
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
    fn preview(&self, rule: &Rule) -> engine::DraftPreview {
        engine::draft_preview(rule, &[self.source.clone()])
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

#[test]
fn unsaved_rule_shows_captures_and_destinations_without_touching_files() {
    let f = Fixture::new();
    let huawei = f.file("报价_华为_2024.xlsx");
    f.file("报价_小米_2023.xlsx");
    f.file("考勤表.xlsx");
    let rule = f.rule(
        vec![regex(r"^报价_(?<客户>[^_]+)_(?<年>\d{4})")],
        vec![
            Action::Rename {
                pattern: "{年}_{name}.{ext}".into(),
            },
            f.organize("{客户}"),
        ],
    );
    let preview = f.preview(&rule);
    assert_eq!((preview.total, preview.matched), (3, 2));
    assert_eq!(preview.unmatched, vec!["考勤表.xlsx".to_string()]);
    assert!(preview.condition_error.is_none() && preview.action_error.is_none());
    let item = preview
        .items
        .iter()
        .find(|item| item.name == "报价_华为_2024.xlsx")
        .unwrap();
    assert_eq!(item.captures["客户"], "华为");
    assert_eq!(item.captures["年"], "2024");
    let kinds: Vec<_> = item.steps.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds, ["rename", "move"]);
    assert_eq!(
        item.steps[1].dest.as_deref().map(PathBuf::from),
        Some(f.target.join("华为").join("2024_报价_华为_2024.xlsx"))
    );
    assert!(huawei.exists() && !f.target.exists());
}

#[test]
fn reports_condition_and_action_problems_instead_of_failing() {
    let f = Fixture::new();
    f.file("a.pdf");
    let bad_regex = f.rule(vec![regex("(")], vec![f.organize("")]);
    let preview = f.preview(&bad_regex);
    assert_eq!(preview.total, 1);
    assert!(preview.condition_error.is_some());
    assert!(preview.items.is_empty());

    let no_dest = f.rule(
        vec![Condition::Extension {
            exts: vec!["pdf".into()],
        }],
        vec![Action::Move { dest: "".into() }],
    );
    let preview = f.preview(&no_dest);
    assert_eq!(preview.matched, 1);
    assert!(preview.action_error.is_some());
    assert!(preview.items[0].steps.is_empty());
}

#[test]
fn reports_missing_sources_and_limits_results() {
    let f = Fixture::new();
    for i in 0..(engine::DRAFT_LIMIT + 5) {
        f.file(&format!("{i:03}.txt"));
    }
    let mut rule = f.rule(
        vec![Condition::Extension {
            exts: vec!["txt".into()],
        }],
        vec![f.organize("")],
    );
    let missing = f.root.join("nope").to_string_lossy().to_string();
    rule.watch_folders.push(missing.clone());
    let preview = engine::draft_preview(&rule, &[f.source.clone(), PathBuf::from(&missing)]);
    assert_eq!(preview.missing_folders, vec![missing]);
    assert_eq!(preview.matched, engine::DRAFT_LIMIT + 5);
    assert_eq!(preview.items.len(), engine::DRAFT_LIMIT);
    let dests: std::collections::HashSet<_> = preview
        .items
        .iter()
        .filter_map(|item| item.steps[0].dest.clone())
        .collect();
    assert_eq!(dests.len(), engine::DRAFT_LIMIT);
}
