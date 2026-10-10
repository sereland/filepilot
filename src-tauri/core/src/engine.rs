//! 执行引擎：扫描文件夹 → 规则匹配 → 生成计划（dry-run）→ 执行计划。
//!
//! 安全原则：
//! - 扫描时跳过临时文件（.tmp/.part/.crdownload）和最近 5 秒内被修改的文件
//!   （避免处理正在下载中的文件——这是此类工具最常见的 bug 来源）。
//! - 删除动作一律走回收站，绝不彻底删除。

use crate::oplog::OpRecord;
use crate::rules::{Action, FileMeta, Rule};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 计划项：dry-run 预览和执行共用
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlanItem {
    /// 源文件
    pub src: String,
    /// 动作中文描述，如 "移动到 D:\文档"
    pub action_desc: String,
    /// 目标路径（重命名/移动/复制），回收站动作无目标
    pub dest: Option<String>,
    /// 动作类型标识：move | copy | rename | recycle
    pub kind: String,
    /// 触发的规则名
    pub rule_name: String,
}

/// 跳过的文件名后缀（下载中的临时文件）
const TEMP_SUFFIXES: &[&str] = &[
    ".tmp",
    ".part",
    ".crdownload",
    ".download",
    ".opdownload",
    ".!ut",
    ".aria2",
];

/// 文件被认为"写入完成"的静默时长
const QUIET_PERIOD: Duration = Duration::from_secs(5);

fn is_temp_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    TEMP_SUFFIXES.iter().any(|s| lower.ends_with(s)) || lower.starts_with('.')
}

fn is_settled(meta: &FileMeta) -> bool {
    match meta.modified {
        Some(t) => t.elapsed().map(|d| d > QUIET_PERIOD).unwrap_or(true),
        None => true,
    }
}

/// 扫描文件夹（非递归），返回可处理的文件元信息
pub fn scan_folder(folder: &Path) -> Vec<FileMeta> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(folder) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let meta = match FileMeta::from_path(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if is_temp_file(&meta.name) || !is_settled(&meta) {
            continue;
        }
        out.push(meta);
    }
    out
}

/// 对单个规则做 dry-run：只计算计划，不执行
pub fn dry_run(rule: &Rule, folders: &[PathBuf]) -> Vec<PlanItem> {
    let mut plan = Vec::new();
    for folder in folders {
        for meta in scan_folder(folder) {
            if !rule.matches(&meta) {
                continue;
            }
            for action in &rule.actions {
                if let Some(item) = plan_for_action(action, &meta, &rule.name) {
                    plan.push(item);
                }
            }
        }
    }
    plan
}

fn plan_for_action(action: &Action, meta: &FileMeta, rule_name: &str) -> Option<PlanItem> {
    let src = meta.path.to_string_lossy().to_string();
    match action {
        Action::Move { dest } | Action::Copy { dest } => {
            let kind = if matches!(action, Action::Move { .. }) {
                "move"
            } else {
                "copy"
            };
            let dest_path = Path::new(dest).join(&meta.name);
            Some(PlanItem {
                src,
                action_desc: action.describe(),
                dest: Some(dest_path.to_string_lossy().to_string()),
                kind: kind.to_string(),
                rule_name: rule_name.to_string(),
            })
        }
        Action::Rename { pattern } => {
            let new_name = render_rename(pattern, meta);
            let dest_path = meta.path.with_file_name(&new_name);
            // 重命名前后相同则跳过
            if dest_path == meta.path {
                return None;
            }
            Some(PlanItem {
                src,
                action_desc: format!("重命名为 {}", new_name),
                dest: Some(dest_path.to_string_lossy().to_string()),
                kind: "rename".to_string(),
                rule_name: rule_name.to_string(),
            })
        }
        Action::MoveToRecycleBin => Some(PlanItem {
            src,
            action_desc: "移入回收站".to_string(),
            dest: None,
            kind: "recycle".to_string(),
            rule_name: rule_name.to_string(),
        }),
    }
}

/// 仅保留预览中勾选的源文件，保留同一文件的全部动作及原顺序。
/// 计划由后端重新扫描生成，不接受前端传入的动作或目标路径。
/// 空名单不会执行任何文件；预览后新增的匹配文件也不会被带入。
pub fn select_plan_sources(plan: &[PlanItem], selected_sources: &[String]) -> Vec<PlanItem> {
    plan.iter()
        .filter(|item| {
            selected_sources
                .iter()
                .any(|src| crate::oplog::paths_equal(Path::new(src), Path::new(&item.src)))
        })
        .cloned()
        .collect()
}

/// 渲染重命名模板，支持变量 {name} {ext} {date} {datetime}
pub fn render_rename(pattern: &str, meta: &FileMeta) -> String {
    let stem = Path::new(&meta.name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let now = chrono::Local::now();
    pattern
        .replace("{name}", &stem)
        .replace("{ext}", &meta.ext)
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{datetime}", &now.format("%Y%m%d-%H%M%S").to_string())
}

/// 执行计划，返回操作记录（用于日志与撤销）
/// batch_id：同一批执行共用一个批次号，撤销时按批次回滚
pub fn apply_plan(plan: &[PlanItem], batch_id: &str, rule_id: &str) -> Vec<OpRecord> {
    let mut records = Vec::new();
    for item in plan {
        let mut trash_id: Option<String> = None;
        let result: std::io::Result<Option<String>> = match item.kind.as_str() {
            "move" | "rename" => {
                let dest = PathBuf::from(item.dest.as_ref().unwrap());
                move_file_safely(&item.src, &dest).map(|d| Some(d.to_string_lossy().to_string()))
            }
            "copy" => {
                let dest = PathBuf::from(item.dest.as_ref().unwrap());
                copy_file_safely(&item.src, &dest).map(|d| Some(d.to_string_lossy().to_string()))
            }
            "recycle" => match crate::oplog::trash_delete_capturing_id(&item.src) {
                Ok(tid) => {
                    trash_id = tid;
                    Ok(None)
                }
                Err(e) => Err(e),
            },
            _ => continue,
        };
        if let Ok(dest) = result {
            records.push(OpRecord::new(
                batch_id,
                rule_id,
                &item.rule_name,
                item.kind.clone(),
                item.src.clone(),
                dest,
                trash_id,
            ));
        }
        // 单个文件失败不中断整批（记录 error 日志即可，P0 简化处理）
    }
    records
}

/// 安全移动：目标目录自动创建；重名自动加编号，绝不覆盖
fn move_file_safely(src: &str, dest: &Path) -> std::io::Result<PathBuf> {
    let final_dest = unique_dest(dest);
    if let Some(parent) = final_dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 跨盘移动时 rename 会失败，fallback 到复制+删除
    if std::fs::rename(src, &final_dest).is_err() {
        std::fs::copy(src, &final_dest)?;
        std::fs::remove_file(src)?;
    }
    Ok(final_dest)
}

fn copy_file_safely(src: &str, dest: &Path) -> std::io::Result<PathBuf> {
    let final_dest = unique_dest(dest);
    if let Some(parent) = final_dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, &final_dest)?;
    Ok(final_dest)
}

/// 目标已存在时自动加编号：report.pdf → report (1).pdf
fn unique_dest(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let parent = dest.parent().unwrap_or_else(|| Path::new(""));
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = dest
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 1.. {
        let candidate = parent.join(format!("{} ({}){}", stem, i, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{format_bytes, Condition, Rule};
    use std::fs;
    use std::time::SystemTime;

    fn setup_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filepilot_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
        // 让文件"静默"：把修改时间设到 10 秒前
        let old = SystemTime::now() - Duration::from_secs(10);
        let t = filetime_set(dir.join(name), old);
        let _ = t;
    }

    fn filetime_set(path: PathBuf, t: SystemTime) -> std::io::Result<()> {
        let f = fs::File::options().write(true).open(&path)?;
        f.set_modified(t)
    }

    fn pdf_rule() -> Rule {
        Rule {
            id: "r1".into(),
            name: "PDF 归档".into(),
            enabled: true,
            watch_folders: vec![],
            conditions: vec![Condition::Extension {
                exts: vec!["pdf".into()],
            }],
            actions: vec![Action::Move {
                dest: "D:/文档".into(),
            }],
        }
    }

    #[test]
    fn dry_run_matches_only_pdfs() {
        let dir = setup_dir("dry_run");
        touch(&dir, "a.pdf", "x");
        touch(&dir, "b.docx", "x");
        touch(&dir, "c.PDF", "x");

        let plan = dry_run(&pdf_rule(), &[dir.clone()]);
        let mut names: Vec<String> = plan
            .iter()
            .map(|p| {
                Path::new(&p.src)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.pdf", "c.PDF"]);
        assert!(plan.iter().all(|p| p.kind == "move"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_temp_and_fresh_files() {
        let dir = setup_dir("skip");
        touch(&dir, "x.tmp", "x");
        touch(&dir, "y.crdownload", "x");
        // 刚写入的文件（5 秒内）应被跳过
        fs::write(dir.join("fresh.pdf"), "x").unwrap();

        let found = scan_folder(&dir);
        assert!(found.is_empty(), "临时文件和未静默文件应被跳过");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_pattern_renders() {
        let meta = FileMeta {
            path: PathBuf::from("/tmp/截图.png"),
            name: "截图.png".into(),
            ext: "png".into(),
            size: 10,
            created: None,
            modified: None,
        };
        let out = render_rename("截图-{date}.{ext}", &meta);
        assert!(out.starts_with("截图-20"));
        assert!(out.ends_with(".png"));
        let out2 = render_rename("{name}_备份.{ext}", &meta);
        assert_eq!(out2, "截图_备份.png");
    }

    #[test]
    fn unique_dest_never_overwrites() {
        let dir = setup_dir("unique");
        touch(&dir, "a.pdf", "old");
        let dest = dir.join("a.pdf");
        let unique = unique_dest(&dest);
        assert_eq!(unique.file_name().unwrap().to_string_lossy(), "a (1).pdf");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_plan_moves_and_records() {
        let dir = setup_dir("apply");
        let dest_dir = dir.join("out");
        touch(&dir, "a.pdf", "data");

        let rule = Rule {
            actions: vec![Action::Move {
                dest: dest_dir.to_string_lossy().to_string(),
            }],
            ..pdf_rule()
        };
        let plan = dry_run(&rule, &[dir.clone()]);
        assert_eq!(plan.len(), 1);
        let records = apply_plan(&plan, "batch1", "r1");
        assert_eq!(records.len(), 1);
        assert!(!dir.join("a.pdf").exists());
        assert!(dest_dir.join("a.pdf").exists());
        assert_eq!(records[0].batch_id, "batch1");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_selection_empty_or_unknown_never_includes_files() {
        let item = PlanItem {
            src: "C:/demo/a.pdf".into(),
            action_desc: "复制".into(),
            dest: Some("C:/backup/a.pdf".into()),
            kind: "copy".into(),
            rule_name: "测试".into(),
        };
        assert!(select_plan_sources(&[item.clone()], &[]).is_empty());
        assert!(select_plan_sources(&[item], &["C:/demo/other.pdf".into()]).is_empty());
    }

    #[test]
    fn preview_selection_keeps_all_actions_in_order() {
        let copy = PlanItem {
            src: "C:/demo/a.pdf".into(),
            action_desc: "复制".into(),
            dest: Some("C:/backup/a.pdf".into()),
            kind: "copy".into(),
            rule_name: "测试".into(),
        };
        let mut move_item = copy.clone();
        move_item.kind = "move".into();
        let mut excluded = copy.clone();
        excluded.src = "C:/demo/b.pdf".into();
        let selected = select_plan_sources(
            &[copy, move_item, excluded],
            &["C:/demo/a.pdf".into(), "C:/demo/a.pdf".into()],
        );
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].kind, "copy");
        assert_eq!(selected[1].kind, "move");
    }

    #[test]
    fn preview_selection_leaves_excluded_and_new_files_untouched() {
        let dir = setup_dir("preview_selection");
        touch(&dir, "chosen.pdf", "chosen");
        touch(&dir, "excluded.pdf", "excluded");
        let rule = Rule {
            actions: vec![
                Action::Copy { dest: dir.join("backup").to_string_lossy().to_string() },
                Action::Move { dest: dir.join("archive").to_string_lossy().to_string() },
            ],
            ..pdf_rule()
        };
        let preview = dry_run(&rule, &[dir.clone()]);
        assert_eq!(preview.len(), 4);
        let selected_sources = vec![dir.join("chosen.pdf").to_string_lossy().to_string()];
        // 预览后出现的新文件符合规则，但不在确认名单中。
        touch(&dir, "new.pdf", "new");
        let plan = select_plan_sources(&dry_run(&rule, &[dir.clone()]), &selected_sources);
        let records = apply_plan(&plan, "selected-batch", "r1");
        assert_eq!(records.len(), 2);
        assert!(!dir.join("chosen.pdf").exists());
        assert_eq!(fs::read_to_string(dir.join("backup/chosen.pdf")).unwrap(), "chosen");
        assert_eq!(fs::read_to_string(dir.join("archive/chosen.pdf")).unwrap(), "chosen");
        for name in ["excluded.pdf", "new.pdf"] {
            assert!(dir.join(name).exists());
            assert!(!dir.join("backup").join(name).exists());
            assert!(!dir.join("archive").join(name).exists());
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_bytes_works() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
