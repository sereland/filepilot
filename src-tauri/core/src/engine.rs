//! 执行引擎：扫描文件夹 → 规则匹配 → 生成计划（dry-run）→ 执行计划。
//!
//! 安全原则：
//! - 扫描时跳过临时文件（.tmp/.part/.crdownload）和最近 5 秒内被修改的文件
//!   （避免处理正在下载中的文件——这是此类工具最常见的 bug 来源）。
//! - 删除动作一律走回收站，绝不彻底删除。

use crate::oplog::OpRecord;
use crate::rules::{Action, ArchiveRetention, FileMeta, Rule, TargetKind};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 计划项：dry-run 预览和执行共用
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlanItem {
    /// 源文件
    pub src: String,
    /// Preview source for this step; src remains the original selection/group key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_src: Option<String>,
    /// 动作中文描述，如 "移动到 D:\文档"
    pub action_desc: String,
    /// 目标路径（重命名/移动/复制），回收站动作无目标
    pub dest: Option<String>,
    /// 动作类型标识：move | copy | rename | recycle | move_extract
    pub kind: String,
    /// 触发的规则名
    pub rule_name: String,
    #[serde(default)]
    pub is_directory: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<crate::archive::ArchivePlan>,
}

/// 跳过的文件名后缀（下载中的临时文件）
pub(crate) const TEMP_SUFFIXES: &[&str] = &[
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
    scan_entries(folder, TargetKind::File)
}

fn scan_entries(folder: &Path, target: TargetKind) -> Vec<FileMeta> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(folder) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if crate::archive::no_links(&path).is_err()
            || (target == TargetKind::Directory && !path.is_dir())
            || (target == TargetKind::File && !path.is_file())
        {
            continue;
        }
        let meta = match FileMeta::from_path(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if is_temp_file(&meta.name)
            || !is_settled(&meta)
            || (meta.is_directory && !crate::directory::settled(&path, QUIET_PERIOD))
        {
            continue;
        }
        out.push(meta);
    }
    out
}

/// 对单个规则做 dry-run：只计算计划，不执行
pub fn dry_run(rule: &Rule, folders: &[PathBuf]) -> Vec<PlanItem> {
    dry_run_at(rule, folders, SystemTime::now())
}

pub fn dry_run_at(rule: &Rule, folders: &[PathBuf], at: SystemTime) -> Vec<PlanItem> {
    dry_run_filtered_at(rule, folders, &|_| true, at)
}

pub fn dry_run_filtered(
    rule: &Rule,
    folders: &[PathBuf],
    include: &dyn Fn(&Path) -> bool,
) -> Vec<PlanItem> {
    dry_run_filtered_at(rule, folders, include, SystemTime::now())
}

fn dry_run_filtered_at(
    rule: &Rule,
    folders: &[PathBuf],
    include: &dyn Fn(&Path) -> bool,
    at: SystemTime,
) -> Vec<PlanItem> {
    let matcher = match rule.compile_matcher() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<_> = folders
        .iter()
        .flat_map(|folder| scan_entries(folder, rule.target_kind))
        .filter(|meta| {
            include(&meta.path)
                && (rule.target_kind == TargetKind::Directory) == meta.is_directory
                && matcher.matches(meta)
                && seen.insert(crate::oplog::normalize_path(&meta.path))
        })
        // A destination container inside the source must not itself be organized.
        .filter(|meta| {
            !meta.is_directory
                || !rule.actions.iter().any(|action| match action {
                    Action::Move { dest }
                    | Action::Copy { dest }
                    | Action::Organize { dest, .. } => Path::new(dest)
                        .ancestors()
                        .any(|ancestor| crate::oplog::paths_equal(ancestor, &meta.path)),
                    _ => false,
                })
        })
        .collect();
    files.sort_by_key(|meta| crate::oplog::normalize_path(&meta.path));
    if rule.target_kind == TargetKind::Directory {
        return files
            .iter()
            .filter_map(|meta| {
                let action = rule.actions.first()?;
                let mut item = plan_for_action(action, meta, &rule.name, at)?;
                if rule.actions.len() != 1 {
                    item.error = Some("文件夹整理目前每条规则支持一个动作。".into());
                }
                Some(item)
            })
            .collect();
    }
    if let Some(Action::MoveAndExtract { dest, retention }) = rule
        .actions
        .iter()
        .find(|a| matches!(a, Action::MoveAndExtract { .. }))
    {
        let groups = if *retention == ArchiveRetention::Latest {
            crate::archive::groups(files)
        } else {
            files.into_iter().map(|file| vec![file]).collect()
        };
        let mut reserved = std::collections::HashSet::new();
        return groups.into_iter().map(|files| {
            let meta = &files[0];
            let obsolete = files.iter().skip(1).map(|file| file.path.to_string_lossy().to_string()).collect();
            let mut archive = crate::archive::plan(meta, dest, retention, obsolete, &mut reserved);
            if rule.actions.len() != 1 { archive.error = Some("移动并解压已包含整个更新流程，请移除其他动作。".into()); }
            if rule.watch_folders.iter().any(|folder| crate::oplog::paths_equal(Path::new(folder), Path::new(dest))) {
                archive.error = Some("目标文件夹不能同时作为此规则的来源。".into());
            }
            if *retention == ArchiveRetention::Latest && files.iter().skip(1).any(|file| {
                file.modified == meta.modified && archive.expected_state.get(&file.path.to_string_lossy().to_string()) != archive.expected_state.get(&meta.path.to_string_lossy().to_string())
            }) { archive.error = Some("同组 ZIP 的修改时间相同且内容不同，无法确定最新一份；请移走旧包或选择保留全部。".into()); }
            PlanItem { src: meta.path.to_string_lossy().to_string(), action_desc: rule.actions.iter().find(|a| matches!(a, Action::MoveAndExtract { .. })).unwrap().describe(),
                dest: Some(archive.folder_dest.clone()), kind: "move_extract".into(), rule_name: rule.name.clone(), is_directory: false, input_src: None, source_digest: None, error: None, archive: Some(archive) }
        }).collect();
    }
    let mut plan = Vec::new();
    let mut reserved = std::collections::HashSet::new();
    for meta in files {
        let origin = meta.path.to_string_lossy().to_string();
        let mut current = meta.clone();
        let digest = if rule.actions.len() > 1 {
            crate::archive::fingerprint(&meta.path).map(Some)
        } else {
            Ok(None)
        };
        for action in &rule.actions {
            if let Some(mut item) = plan_for_action(action, &current, &rule.name, at) {
                item.input_src = Some(current.path.to_string_lossy().to_string());
                item.src = origin.clone();
                item.source_digest = digest.as_ref().ok().cloned().flatten();
                if let Err(error) = &digest {
                    item.error = Some(format!("无法校验来源文件：{error}"));
                }
                if let Some(dest) = &item.dest {
                    let mut candidate = unique_dest(Path::new(dest));
                    let mut n = 1;
                    while reserved.contains(&crate::oplog::normalize_path(&candidate))
                        || candidate.exists()
                    {
                        candidate = numbered_dest(Path::new(dest), n);
                        n += 1;
                    }
                    reserved.insert(crate::oplog::normalize_path(&candidate));
                    item.dest = Some(candidate.to_string_lossy().to_string());
                    if item.error.is_none() && matches!(item.kind.as_str(), "move" | "rename") {
                        current.path = candidate;
                        current.name = current
                            .path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_string();
                        current.ext = current
                            .path
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                    }
                }
                let stop = item.error.is_some() || item.kind == "recycle";
                plan.push(item);
                if stop {
                    break;
                }
            }
        }
    }
    plan
}

fn plan_for_action(
    action: &Action,
    meta: &FileMeta,
    rule_name: &str,
    at: SystemTime,
) -> Option<PlanItem> {
    let resolved = match crate::templates::resolve_at(action, meta, at) {
        Ok(action) => action,
        Err(error) => {
            return Some(PlanItem {
                src: meta.path.to_string_lossy().to_string(),
                input_src: None,
                action_desc: action.describe(),
                dest: None,
                kind: "invalid".into(),
                rule_name: rule_name.into(),
                is_directory: meta.is_directory,
                source_digest: None,
                error: Some(error),
                archive: None,
            })
        }
    };
    let action = &resolved;
    if meta.is_directory {
        let (kind, dest) = match action {
            Action::Move { dest } => ("move", Some(Path::new(dest).join(&meta.name))),
            Action::Copy { dest } => ("copy", Some(Path::new(dest).join(&meta.name))),
            Action::Rename { pattern } => {
                let name = pattern.clone();
                if name == meta.name {
                    return None;
                }
                if !crate::archive::valid_component(&name) {
                    return Some(PlanItem {
                        src: meta.path.to_string_lossy().to_string(),
                        dest: None,
                        action_desc: action.describe(),
                        kind: "rename".into(),
                        rule_name: rule_name.into(),
                        is_directory: true,
                        input_src: None,
                        source_digest: None,
                        error: Some("文件夹名称无效，不能包含路径或特殊字符。".into()),
                        archive: None,
                    });
                }
                ("rename", Some(meta.path.with_file_name(name)))
            }
            Action::MoveToRecycleBin => ("recycle", None),
            Action::MoveAndExtract { .. } => return None,
            Action::Organize { .. } | Action::RenameWithDate { .. } => unreachable!(),
        };
        let dest = dest.map(|p| crate::directory::unique_destination(&p));
        let digest = crate::archive::fingerprint(&meta.path);
        let error = digest.as_ref().err().map(ToString::to_string).or_else(|| {
            dest.as_ref()
                .and_then(|p| crate::directory::validate_destination(&meta.path, p).err())
                .map(|e| e.to_string())
        });
        return Some(PlanItem {
            src: meta.path.to_string_lossy().to_string(),
            action_desc: action.describe(),
            dest: dest.map(|p| p.to_string_lossy().to_string()),
            kind: kind.into(),
            rule_name: rule_name.into(),
            is_directory: true,
            input_src: None,
            source_digest: digest.ok(),
            error,
            archive: None,
        });
    }
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
                is_directory: false,
                input_src: None,
                source_digest: None,
                error: None,
                archive: None,
            })
        }
        Action::Rename { pattern } => {
            let new_name = pattern.clone();
            if !crate::archive::valid_component(&new_name) {
                return Some(PlanItem {
                    src,
                    input_src: None,
                    action_desc: action.describe(),
                    dest: None,
                    kind: "rename".into(),
                    rule_name: rule_name.into(),
                    is_directory: false,
                    source_digest: None,
                    error: Some("文件名无效，不能包含路径或特殊字符。".into()),
                    archive: None,
                });
            }
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
                is_directory: false,
                input_src: None,
                source_digest: None,
                error: None,
                archive: None,
            })
        }
        Action::MoveToRecycleBin => Some(PlanItem {
            src,
            action_desc: "移入回收站".to_string(),
            dest: None,
            kind: "recycle".to_string(),
            rule_name: rule_name.to_string(),
            is_directory: false,
            input_src: None,
            source_digest: None,
            error: None,
            archive: None,
        }),
        Action::MoveAndExtract { .. } => None,
        Action::Organize { .. } | Action::RenameWithDate { .. } => unreachable!(),
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

/// Ordinary files reserve names against the whole scan before selection, so
/// excluding an earlier file does not silently change another file's preview.
/// Archive groups and directories continue filtering before planning.
pub fn selected_dry_run(
    rule: &Rule,
    folders: &[PathBuf],
    selected_sources: &[String],
) -> Vec<PlanItem> {
    selected_dry_run_at(rule, folders, selected_sources, SystemTime::now())
}

pub fn selected_dry_run_at(
    rule: &Rule,
    folders: &[PathBuf],
    selected_sources: &[String],
    at: SystemTime,
) -> Vec<PlanItem> {
    if rule.target_kind == TargetKind::File
        && !rule
            .actions
            .iter()
            .any(|a| matches!(a, Action::MoveAndExtract { .. }))
    {
        select_plan_sources(&dry_run_at(rule, folders, at), selected_sources)
    } else {
        dry_run_filtered_at(
            rule,
            folders,
            &|path| {
                selected_sources
                    .iter()
                    .any(|src| crate::oplog::paths_equal(Path::new(src), path))
            },
            at,
        )
    }
}

pub fn confirmed_dry_run(
    rule: &Rule,
    folders: &[PathBuf],
    selected_sources: &[String],
    expected: Option<&[PlanItem]>,
    at: SystemTime,
) -> Result<Vec<PlanItem>, String> {
    let plan = selected_dry_run_at(rule, folders, selected_sources, at);
    if let Some(expected) = expected {
        if serde_json::to_value(&plan).map_err(|e| e.to_string())?
            != serde_json::to_value(expected).map_err(|e| e.to_string())?
        {
            return Err("文件或目标内容已变化，请重新预览后再整理。".into());
        }
    } else if plan.iter().any(|p| p.archive.is_some() || p.is_directory) {
        return Err("请先预览整理效果。".into());
    }
    Ok(plan)
}

/// 渲染重命名模板，支持变量 {name} {ext} {date} {datetime}
pub fn render_rename(pattern: &str, meta: &FileMeta) -> String {
    crate::templates::render(pattern, meta, crate::rules::DateSource::Now)
        .unwrap_or_else(|_| pattern.into())
}

/// 执行计划，返回操作记录（用于日志与撤销）
/// batch_id：同一批执行共用一个批次号，撤销时按批次回滚
pub fn apply_plan(plan: &[PlanItem], batch_id: &str, rule_id: &str) -> Vec<OpRecord> {
    let mut records = Vec::new();
    let mut current_paths: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut failed = std::collections::HashSet::new();
    for item in plan {
        if item.is_directory {
            let mut record = OpRecord::new(
                batch_id,
                rule_id,
                &item.rule_name,
                item.kind.clone(),
                item.src.clone(),
                item.dest.clone(),
                None,
            );
            record.is_directory = true;
            record.directory_digest = item.source_digest.clone();
            let result = (|| -> std::io::Result<()> {
                if let Some(error) = &item.error {
                    return Err(std::io::Error::other(error.clone()));
                }
                let source = Path::new(&item.src);
                let expected = item
                    .source_digest
                    .as_deref()
                    .ok_or_else(|| std::io::Error::other("缺少文件夹预览状态。"))?;
                if !source.is_dir()
                    || crate::archive::fingerprint(source)? != expected
                    || !crate::directory::settled(source, QUIET_PERIOD)
                {
                    return Err(std::io::Error::other(
                        "来源文件夹已变化或仍在写入，请稍后重新预览。",
                    ));
                }
                match item.kind.as_str() {
                    "move" | "rename" => crate::directory::move_to(
                        source,
                        Path::new(item.dest.as_ref().unwrap()),
                        expected,
                    ),
                    "copy" => crate::directory::copy_to(
                        source,
                        Path::new(item.dest.as_ref().unwrap()),
                        expected,
                    ),
                    "recycle" => {
                        record.trash_id = crate::oplog::trash_delete_capturing_id(&item.src)?;
                        Ok(())
                    }
                    _ => Err(std::io::Error::other("不支持的文件夹动作。")),
                }
            })();
            if let Err(e) = result {
                record.error = Some(e.to_string());
                record.undo_unavailable = Some("未执行成功".into());
            }
            records.push(record);
            continue;
        }
        if let Some(archive) = &item.archive {
            let mut record = OpRecord::new(
                batch_id,
                rule_id,
                &item.rule_name,
                "move_extract".into(),
                item.src.clone(),
                Some(archive.folder_dest.clone()),
                None,
            );
            match crate::archive::execute_record(&item.src, archive, &record) {
                Ok((manifest, warning)) => {
                    record.archive_manifest = Some(manifest);
                    record.warning = warning;
                }
                Err(e) => {
                    record.error = Some(e.to_string());
                    record.undo_unavailable = Some("未执行成功".into());
                    record.restored_sources = std::iter::once(item.src.clone())
                        .chain(archive.obsolete_sources.iter().cloned())
                        .collect();
                }
            }
            records.push(record);
            continue;
        }
        let key = crate::oplog::normalize_path(Path::new(&item.src));
        if failed.contains(&key) {
            continue;
        }
        let src = current_paths
            .get(&key)
            .cloned()
            .unwrap_or_else(|| item.src.clone());
        let mut record = OpRecord::new(
            batch_id,
            rule_id,
            &item.rule_name,
            item.kind.clone(),
            src.clone(),
            item.dest.clone(),
            None,
        );
        if item.input_src.is_some() {
            record.origin_src = Some(item.src.clone());
        }
        record.file_digest = item.source_digest.clone();
        let mut trash_id: Option<String> = None;
        let result: std::io::Result<Option<String>> = (|| {
            if let Some(error) = &item.error {
                return Err(std::io::Error::other(error.clone()));
            }
            if !Path::new(&src).is_file() {
                return Err(std::io::Error::other("来源文件已不存在，请重新预览。"));
            }
            if let Some(expected) = &item.source_digest {
                if crate::archive::fingerprint(Path::new(&src))? != *expected {
                    return Err(std::io::Error::other("来源内容已变化，请重新预览。"));
                }
            }
            if let Some(dest) = &item.dest {
                if item.input_src.is_some() && Path::new(dest).exists() {
                    return Err(std::io::Error::other("目标已被占用，请重新预览。"));
                }
            }
            match item.kind.as_str() {
                "move" | "rename" => {
                    let dest = PathBuf::from(item.dest.as_ref().unwrap());
                    move_file_safely(&src, &dest).map(|d| Some(d.to_string_lossy().to_string()))
                }
                "copy" => {
                    let dest = PathBuf::from(item.dest.as_ref().unwrap());
                    copy_file_safely(&src, &dest).map(|d| Some(d.to_string_lossy().to_string()))
                }
                "recycle" => match crate::oplog::trash_delete_capturing_id(&src) {
                    Ok(tid) => {
                        trash_id = tid;
                        Ok(None)
                    }
                    Err(e) => Err(e),
                },
                _ => Err(std::io::Error::other("不支持的动作。")),
            }
        })();
        match result {
            Ok(dest) => {
                record.dest = dest.clone();
                record.trash_id = trash_id;
                if matches!(item.kind.as_str(), "move" | "rename") {
                    if let Some(path) = dest {
                        current_paths.insert(key, path);
                    }
                } else if item.kind == "recycle" {
                    failed.insert(key);
                }
            }
            Err(error) => {
                record.error = Some(error.to_string());
                record.undo_unavailable = Some("未执行成功".into());
                failed.insert(key);
            }
        }
        records.push(record);
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

fn numbered_dest(dest: &Path, n: usize) -> PathBuf {
    let stem = dest.file_stem().unwrap_or_default().to_string_lossy();
    let ext = dest
        .extension()
        .map(|v| format!(".{}", v.to_string_lossy()))
        .unwrap_or_default();
    dest.with_file_name(format!("{stem} ({n}){ext}"))
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
            target_kind: crate::rules::TargetKind::File,
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
            is_directory: false,
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
            is_directory: false,
            input_src: None,
            source_digest: None,
            error: None,
            archive: None,
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
            is_directory: false,
            input_src: None,
            source_digest: None,
            error: None,
            archive: None,
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
                Action::Copy {
                    dest: dir.join("backup").to_string_lossy().to_string(),
                },
                Action::Move {
                    dest: dir.join("archive").to_string_lossy().to_string(),
                },
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
        assert_eq!(
            fs::read_to_string(dir.join("backup/chosen.pdf")).unwrap(),
            "chosen"
        );
        assert_eq!(
            fs::read_to_string(dir.join("archive/chosen.pdf")).unwrap(),
            "chosen"
        );
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
