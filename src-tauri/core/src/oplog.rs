//! 操作日志：每次执行以 JSONL 追加记录，支持按批次撤销。
//!
//! 撤销语义：
//! - move/rename：把文件移回原路径（原位置被占用则自动加编号，绝不覆盖）
//! - copy：删除复制出的副本
//! - recycle：按回收站条目 id 恢复，旧记录按来源路径匹配
//! - move_extract：恢复上一版 ZIP、解压目录和同组下载包

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OpRecord {
    pub id: String,
    pub batch_id: String,
    pub timestamp: String,
    pub rule_id: String,
    pub rule_name: String,
    /// move | copy | rename | recycle | move_extract
    pub kind: String,
    pub src: String,
    /// 执行后的实际目标路径（重名加编号后的最终路径）
    pub dest: Option<String>,
    pub undone: bool,
    #[serde(default)]
    pub is_directory: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_digest: Option<String>,
    /// 回收站条目 id（仅 recycle，用于撤销时精确恢复）；旧记录为 None
    #[serde(default)]
    pub trash_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_unavailable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restored_sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restored_state: Vec<(String, u64, std::time::SystemTime)>,
}

impl OpRecord {
    pub fn new(
        batch_id: &str,
        rule_id: &str,
        rule_name: &str,
        kind: String,
        src: String,
        dest: Option<String>,
        trash_id: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            batch_id: batch_id.to_string(),
            timestamp: Local::now().to_rfc3339(),
            rule_id: rule_id.to_string(),
            rule_name: rule_name.to_string(),
            kind,
            src,
            dest,
            undone: false,
            is_directory: false,
            directory_digest: None,
            trash_id,
            archive_manifest: None,
            undo_unavailable: None,
            error: None,
            warning: None,
            restored_sources: Vec::new(),
            restored_state: Vec::new(),
        }
    }
}

/// 追加一批记录到 JSONL 日志
pub fn append_records(log_path: &Path, records: &[OpRecord]) -> std::io::Result<()> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    for r in records {
        writeln!(f, "{}", serde_json::to_string(r).unwrap())?;
    }
    Ok(())
}

/// Package journals can survive a process exit before the separate JSONL write.
pub fn reconcile_archive_records(log_path: &Path, recovered: &[OpRecord]) -> std::io::Result<()> {
    if recovered.is_empty() {
        return Ok(());
    }
    let mut all = read_all(log_path);
    let mut changed = false;
    for record in recovered {
        if let Some(existing) = all.iter_mut().find(|r| r.id == record.id) {
            if record.undone && !existing.undone {
                *existing = record.clone();
                changed = true;
            } else if !existing.undone && existing.warning != record.warning {
                existing.warning = record.warning.clone();
                changed = true;
            }
        } else {
            all.push(record.clone());
            changed = true;
        }
    }
    if changed {
        all.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        rewrite_all(log_path, &all)?;
    }
    Ok(())
}

/// 读取全部记录（最新的在后）
pub fn read_all(log_path: &Path) -> Vec<OpRecord> {
    let f = match std::fs::File::open(log_path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    BufReader::new(f)
        .lines()
        .filter_map(|l| l.ok())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect()
}

/// 读取最近 N 条（最新的在前）
pub fn read_recent(log_path: &Path, limit: usize) -> Vec<OpRecord> {
    let mut all = read_all(log_path);
    all.reverse();
    all.truncate(limit);
    all.into_iter().map(with_undo_state).collect()
}

fn with_undo_state(mut record: OpRecord) -> OpRecord {
    if !record.undone {
        if let Some(path) = &record.archive_manifest {
            record.undo_unavailable = crate::archive::undo_reason(path);
        }
    }
    record
}

#[derive(Serialize, Clone, Debug)]
pub struct RuleLastRun {
    pub batch_id: String,
    pub timestamp: String,
    pub file_count: usize,
    pub directory_count: usize,
}

/// 每条规则最近一次实际整理的批次、时间和文件数，不受记录页条数限制影响。
/// 已撤销的操作仍算执行过，撤销不会把规则变回“暂未执行”。
pub fn rule_last_runs(log_path: &Path) -> std::collections::HashMap<String, RuleLastRun> {
    let records = read_all(log_path);
    let mut last_runs = std::collections::HashMap::new();
    for record in records.iter().filter(|r| r.error.is_none()) {
        last_runs.insert(
            record.rule_id.clone(),
            RuleLastRun {
                batch_id: record.batch_id.clone(),
                timestamp: record.timestamp.clone(),
                file_count: 0,
                directory_count: 0,
            },
        );
    }
    let mut sources: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    let mut directories: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for record in records.iter().filter(|r| r.error.is_none()) {
        if last_runs
            .get(&record.rule_id)
            .map(|run| run.batch_id == record.batch_id)
            .unwrap_or(false)
        {
            if record.is_directory {
                directories
                    .entry(record.rule_id.clone())
                    .or_default()
                    .insert(normalize_path(Path::new(&record.src)));
            }
            sources
                .entry(record.rule_id.clone())
                .or_default()
                .insert(normalize_path(Path::new(&record.src)));
        }
    }
    for (rule_id, run) in &mut last_runs {
        run.file_count = sources.get(rule_id).map(|files| files.len()).unwrap_or(0);
        run.directory_count = directories
            .get(rule_id)
            .map(|items| items.len())
            .unwrap_or(0);
    }
    last_runs
}

/// 按批次取完整记录，用于从规则卡片定位，不受最近 N 项限制。
pub fn read_batch(log_path: &Path, batch_id: &str) -> Vec<OpRecord> {
    read_all(log_path)
        .into_iter()
        .rev()
        .filter(|record| record.batch_id == batch_id)
        .map(with_undo_state)
        .collect()
}

/// 撤销最近一个未撤销的批次，返回被撤销的记录。
/// 批次内按逆序撤销（后执行的先回滚）。
pub fn undo_last_batch(log_path: &Path) -> std::io::Result<Vec<OpRecord>> {
    let mut all = read_all(log_path);
    // 找到最近的未撤销批次
    let target_batch = all
        .iter()
        .rev()
        .find(|r| !r.undone && with_undo_state((*r).clone()).undo_unavailable.is_none())
        .map(|r| r.batch_id.clone());
    let batch_id = match target_batch {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    let mut undone = Vec::new();
    let mut last_error = None;
    for record in all.iter_mut().rev() {
        if record.batch_id != batch_id
            || record.undone
            || with_undo_state(record.clone()).undo_unavailable.is_some()
        {
            continue;
        }
        if record.kind == "move_extract" {
            let path = record
                .archive_manifest
                .as_ref()
                .ok_or_else(|| std::io::Error::other("缺少包更新记录"))?;
            match crate::archive::undo(path) {
                Ok((paths, states)) => {
                    record.restored_sources = paths
                        .into_iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    record.restored_state = states;
                }
                Err(e) => {
                    if undone.is_empty() {
                        return Err(e);
                    }
                    rewrite_all(log_path, &all)?;
                    return Ok(undone);
                }
            }
            record.undone = true;
            undone.push(record.clone());
        } else {
            match undo_one(record) {
                Ok(()) => {
                    record.undone = true;
                    undone.push(record.clone());
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
        }
    }

    // 写回整份日志（P0 数据量小，全量重写可接受）
    rewrite_all(log_path, &all)?;
    if undone.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
    }
    Ok(undone)
}

fn undo_one(record: &mut OpRecord) -> std::io::Result<()> {
    if record.is_directory {
        if record.kind == "recycle" {
            return restore_from_trash(record);
        }
        let dest = Path::new(
            record
                .dest
                .as_deref()
                .ok_or_else(|| std::io::Error::other("缺少目标文件夹。"))?,
        );
        let digest = record
            .directory_digest
            .as_deref()
            .ok_or_else(|| std::io::Error::other("缺少文件夹校验记录。"))?;
        if !dest.is_dir() || crate::archive::fingerprint(dest)? != digest {
            return Err(std::io::Error::other(
                "目标文件夹已变化，停止撤销以保留当前内容。",
            ));
        }
        match record.kind.as_str() {
            "move" | "rename" => {
                let back = crate::directory::unique_destination(Path::new(&record.src));
                crate::directory::move_to(dest, &back, digest)?;
                record.restored_sources = vec![back.to_string_lossy().to_string()];
                return Ok(());
            }
            "copy" => {
                // A complete directory enters Trash, avoiding partial recursive deletion.
                trash::delete(dest).map_err(|e| std::io::Error::other(e.to_string()))?;
                record.restored_sources = vec![record.src.clone()];
                return Ok(());
            }
            _ => return Err(std::io::Error::other("不支持的文件夹撤销操作。")),
        }
    }
    match record.kind.as_str() {
        "move" | "rename" => {
            let dest =
                Path::new(record.dest.as_ref().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::Other, "missing dest")
                })?);
            if !dest.exists() {
                return Ok(()); // 目标已不在，可能被用户手动处理过，跳过
            }
            let back = unique_back(Path::new(&record.src));
            if let Some(parent) = back.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if std::fs::rename(dest, &back).is_err() {
                std::fs::copy(dest, &back)?;
                std::fs::remove_file(dest)?;
            }
            record.restored_sources = vec![back.to_string_lossy().to_string()];
            Ok(())
        }
        "copy" => {
            // 撤销复制 = 删除副本
            if let Some(dest) = &record.dest {
                let p = Path::new(dest);
                if p.exists() {
                    std::fs::remove_file(p)?;
                }
            }
            Ok(())
        }
        "recycle" => restore_from_trash(record),
        _ => Ok(()),
    }
}

/// 删除到回收站，并捕获回收站条目的 id（用于撤销时精确恢复）。
/// 返回 Ok(None) 表示删除成功但未能捕获 id（调用方可回退到路径匹配）。
/// 通过删除前后的 id 差集定位本次删除的条目，不依赖路径字符串比对。
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
pub(crate) fn trash_delete_capturing_id(src: &str) -> std::io::Result<Option<String>> {
    use std::collections::HashSet;
    use std::ffi::OsString;
    let before: HashSet<OsString> = trash::os_limited::list()
        .map(|v| v.into_iter().map(|i| i.id).collect())
        .unwrap_or_default();
    trash::delete(src)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    // Deletion already succeeded. If listing fails, keep the operation record and
    // fall back to original-path matching on undo instead of losing the record.
    let after = match list_trash_items() {
        Ok(items) => items,
        Err(_) => return Ok(None),
    };
    Ok(after
        .into_iter()
        .find(|i| !before.contains(&i.id) && paths_equal(&i.original_path(), Path::new(src)))
        .map(|i| i.id.to_string_lossy().to_string()))
}

/// 不支持 os_limited 的平台：普通删除，不捕获 id
#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
pub(crate) fn trash_delete_capturing_id(src: &str) -> std::io::Result<Option<String>> {
    trash::delete(src)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(None)
}

/// 从回收站恢复指定记录的文件。
/// 优先使用删除时捕获的 trash id 精确匹配；兜底按原始路径匹配（兼容旧记录）。
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn restore_from_trash(record: &OpRecord) -> std::io::Result<()> {
    let items = list_trash_items()?;
    // 1) 按 trash id 精确恢复
    if let Some(tid) = record.trash_id.as_deref() {
        if let Some(item) = items.iter().find(|i| i.id.to_string_lossy() == tid) {
            return restore_trash_item(item.clone(), Path::new(&record.src));
        }
    }
    // 2) 兜底：按原始路径匹配
    let src_path = Path::new(&record.src);
    let mut candidates: Vec<_> = items
        .into_iter()
        .filter(|it| paths_equal(&it.original_path(), src_path))
        .collect();
    if candidates.is_empty() {
        if src_path.exists() {
            return Ok(()); // 文件已在原位置（用户手动恢复过）
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("在回收站中找不到 {}，请手动从系统回收站恢复", record.src),
        ));
    }
    // 逐个恢复：同名双胞胎一次只恢复一个，避免 RestoreTwins 错误
    let mut last_err = None;
    while let Some(item) = candidates.pop() {
        match restore_trash_item(item, src_path) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!(
            "从回收站恢复失败: {}",
            last_err.map(|e| e.to_string()).unwrap_or_default()
        ),
    ))
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn restore_trash_item(item: trash::TrashItem, source: &Path) -> std::io::Result<()> {
    if !paths_equal(&item.original_path(), source) {
        return Err(std::io::Error::other(
            "回收站条目与记录中的原路径不一致，已停止恢复",
        ));
    }
    trash::os_limited::restore_all(std::iter::once(item))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    if !source.exists() {
        return Err(std::io::Error::other(format!(
            "系统返回恢复成功，但原路径不存在：{}",
            source.display()
        )));
    }
    Ok(())
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn list_trash_items() -> std::io::Result<Vec<trash::TrashItem>> {
    let items = trash::os_limited::list().map_err(|e| std::io::Error::other(e.to_string()))?;
    #[cfg(windows)]
    let items = {
        let mut items = items;
        accurate_recycle_names(&mut items)?;
        items
    };
    Ok(items)
}

/// trash 5 uses SIGDN_PARENTRELATIVE, which can omit registered extensions.
/// Query PKEY_FileName on the recycle-bin namespace item, where it is the original
/// name. Reopening its physical $R path would yield the internal storage name.
#[cfg(windows)]
fn accurate_recycle_names(items: &mut [trash::TrashItem]) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::Interface;
    use windows::Win32::Storage::EnhancedStorage::PKEY_FileName;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::*;
    // list() above initializes COM on this thread. Shell owns the returned string
    // until copied; free it with the allocator specified by IShellItem2::GetString.
    unsafe {
        let bin: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        let enumerator: IEnumShellItems = bin
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut pending: std::collections::HashMap<_, _> = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.id.clone(), index))
            .collect();
        loop {
            let mut fetched = 0;
            let mut values = [None];
            enumerator
                .Next(&mut values, Some(&mut fetched))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if fetched == 0 {
                break;
            }
            let item = values[0]
                .take()
                .ok_or_else(|| std::io::Error::other("无法读取回收站条目"))?;
            let value = item
                .GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            let id = std::ffi::OsString::from_wide(value.as_wide());
            CoTaskMemFree(Some(value.0.cast()));
            if let Some(index) = pending.remove(&id) {
                let item: IShellItem2 = item
                    .cast()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let value = item
                    .GetString(&PKEY_FileName)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let name = std::ffi::OsString::from_wide(value.as_wide());
                CoTaskMemFree(Some(value.0.cast()));
                if name.is_empty()
                    || Path::new(&name).components().count() != 1
                    || Path::new(&name).file_name() != Some(name.as_os_str())
                {
                    return Err(std::io::Error::other("回收站原始文件名无效"));
                }
                items[index].name = name;
            }
        }
        if !pending.is_empty() {
            return Err(std::io::Error::other("回收站内容已变化，请重新尝试撤销"));
        }
        Ok(())
    }
}

/// 不支持 os_limited 的平台：提示手动恢复
#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
fn restore_from_trash(_record: &OpRecord) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "当前平台不支持自动恢复，请手动从系统回收站恢复",
    ))
}

/// Windows 短名/长名、verbatim 前缀、分隔符与大小写统一后比较。
/// 文件已进入回收站时，规范化仍存在的祖先，再接回缺失的文件名。
pub fn normalize_path(p: &Path) -> String {
    #[cfg(windows)]
    let resolved = resolve_comparison_path(p);
    #[cfg(windows)]
    let p = resolved.as_path();
    let mut s = p.as_os_str().to_string_lossy().replace('/', "\\");
    #[cfg(target_os = "windows")]
    {
        s = s.to_lowercase();
    }
    if let Some(rest) = s.strip_prefix(r"\\?\unc\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    s
}

#[cfg(windows)]
fn resolve_comparison_path(path: &Path) -> std::path::PathBuf {
    if !path.is_absolute() {
        return path.to_path_buf();
    }
    let mut current = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(current) {
            for component in missing.iter().rev() {
                resolved.push(component);
            }
            return resolved;
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                current = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

pub fn paths_equal(a: &Path, b: &Path) -> bool {
    normalize_path(a) == normalize_path(b)
}

/// 回滚时原位置被占用则加编号，绝不覆盖用户文件
fn unique_back(src: &Path) -> std::path::PathBuf {
    if !src.exists() {
        return src.to_path_buf();
    }
    let parent = src.parent().unwrap_or_else(|| Path::new(""));
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = src
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 1.. {
        let candidate = parent.join(format!("{} (还原{}){}", stem, i, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn rewrite_all(log_path: &Path, records: &[OpRecord]) -> std::io::Result<()> {
    let temp = log_path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp)?;
    for r in records {
        writeln!(f, "{}", serde_json::to_string(r).unwrap())?;
    }
    f.sync_all()?;
    drop(f);
    std::fs::rename(temp, log_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_log(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("filepilot_oplog_{}.jsonl", name));
        let _ = fs::remove_file(&p);
        p
    }

    #[cfg(windows)]
    fn short_path(path: &Path) -> std::path::PathBuf {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetShortPathNameW;
        let input: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let required = unsafe { GetShortPathNameW(PCWSTR(input.as_ptr()), None) };
        assert!(
            required > 0,
            "读取测试目录短名失败：{}",
            std::io::Error::last_os_error()
        );
        let mut buffer = vec![0u16; required as usize];
        let written = unsafe { GetShortPathNameW(PCWSTR(input.as_ptr()), Some(&mut buffer)) };
        assert!(written > 0 && written < required);
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written as usize]))
    }

    #[test]
    #[cfg(windows)]
    fn recycle_undo_matches_short_and_long_parent_paths() {
        let dir = std::env::temp_dir().join(format!(
            "filepilot_long_path_regression_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let long_dir = fs::canonicalize(&dir).unwrap();
        let short_dir = short_path(&long_dir);
        let has_alias = long_dir.as_os_str().to_string_lossy().to_lowercase()
            != short_dir.as_os_str().to_string_lossy().to_lowercase();
        if !has_alias {
            fs::remove_dir_all(&dir).unwrap();
            assert!(
                std::env::var("FILEPILOT_REQUIRE_SHORT_PATH_TEST")
                    .ok()
                    .as_deref()
                    != Some("1"),
                "测试卷未提供 8.3 短名，请在支持短名的临时目录运行此测试"
            );
            eprintln!("本卷未提供 8.3 短名；短路径回收站回归未执行");
            return;
        }
        for capture_id in [false, true] {
            let src = dir.join("original-setup.exe");
            let short_src = short_dir.join("original-setup.exe");
            fs::write(&src, "short path fixture").unwrap();
            assert!(paths_equal(&src, &short_src), "删除前应匹配长短路径");
            let id = trash_delete_capturing_id(short_src.to_str().unwrap()).unwrap();
            assert!(id.is_some(), "短路径删除也必须捕获原文件条目 id");
            assert!(
                paths_equal(&long_dir.join("original-setup.exe"), &short_src),
                "删除后应通过仍存在的父目录匹配"
            );
            let record = OpRecord::new(
                "b",
                "r",
                "测试",
                "recycle".into(),
                short_src.to_string_lossy().to_string(),
                None,
                if capture_id { id } else { None },
            );
            restore_from_trash(&record).unwrap();
            assert_eq!(fs::read_to_string(&src).unwrap(), "short path fixture");
            fs::remove_file(src).unwrap();
        }
        assert!(paths_equal(
            &long_dir.join("missing/nested/file.zip"),
            &short_dir.join("missing/nested/file.zip")
        ));
        assert!(!paths_equal(
            &long_dir.join("missing/a.zip"),
            &short_dir.join("missing/b.zip")
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rule_last_runs_includes_old_and_undone_records() {
        let log = tmp_log("rule_last_runs");
        let mut old = OpRecord::new(
            "old",
            "older-rule",
            "旧规则",
            "copy".into(),
            "a".into(),
            Some("b".into()),
            None,
        );
        old.timestamp = "2026-01-01T10:00:00+08:00".into();
        old.undone = true;
        let mut records = vec![old];
        for index in 0..120 {
            let mut record = OpRecord::new(
                "new",
                "recent-rule",
                "新规则",
                "copy".into(),
                "a".into(),
                Some("b".into()),
                None,
            );
            record.timestamp = format!("time-{}", index);
            records.push(record);
        }
        append_records(&log, &records).unwrap();
        assert!(read_recent(&log, 100)
            .iter()
            .all(|r| r.rule_id == "recent-rule"));
        let last = rule_last_runs(&log);
        assert_eq!(
            last.get("older-rule").unwrap().timestamp,
            "2026-01-01T10:00:00+08:00"
        );
        assert_eq!(last.get("older-rule").unwrap().file_count, 1);
        assert_eq!(last.get("recent-rule").unwrap().timestamp, "time-119");
        let old_batch = read_batch(&log, "old");
        assert_eq!(old_batch.len(), 1);
        assert_eq!(old_batch[0].rule_id, "older-rule");
        assert!(!last.contains_key("never-run"));
        let _ = fs::remove_file(&log);
    }

    #[test]
    fn rule_last_runs_counts_unique_sources_in_latest_batch() {
        let log = tmp_log("rule_last_runs_count");
        let old = OpRecord::new(
            "old",
            "r1",
            "图片归档",
            "move".into(),
            "old.png".into(),
            Some("archive/old.png".into()),
            None,
        );
        let mut records = vec![old];
        for index in 0..12 {
            let src = format!("C:/Demo/{}.png", index);
            for kind in ["copy", "move"] {
                let mut record = OpRecord::new(
                    "latest",
                    "r1",
                    "图片归档",
                    kind.into(),
                    if kind == "copy" {
                        src.clone()
                    } else {
                        src.replace('/', "\\")
                    },
                    Some(format!("archive/{}.png", index)),
                    None,
                );
                record.timestamp = "2026-10-11T10:42:00+08:00".into();
                record.undone = true;
                records.push(record);
            }
        }
        append_records(&log, &records).unwrap();
        let last = rule_last_runs(&log);
        let run = last.get("r1").unwrap();
        assert_eq!(run.batch_id, "latest");
        assert_eq!(run.file_count, 12);
        assert_eq!(read_batch(&log, "latest").len(), 24);
        assert!(read_batch(&log, "missing").is_empty());
        let _ = fs::remove_file(&log);
    }

    #[test]
    fn rule_last_runs_batch_lookup_is_complete_beyond_recent_limit() {
        let log = tmp_log("rule_last_runs_full_batch");
        let records: Vec<_> = (0..240)
            .map(|index| {
                OpRecord::new(
                    "large-batch",
                    "r1",
                    "图片归档",
                    "copy".into(),
                    format!("{}.png", index),
                    Some(format!("out/{}.png", index)),
                    None,
                )
            })
            .collect();
        append_records(&log, &records).unwrap();
        assert_eq!(read_recent(&log, 100).len(), 100);
        assert_eq!(read_batch(&log, "large-batch").len(), 240);
        assert_eq!(rule_last_runs(&log).get("r1").unwrap().file_count, 240);
        let _ = fs::remove_file(&log);
    }

    #[test]
    fn append_and_read_roundtrip() {
        let log = tmp_log("rt");
        let r = OpRecord::new(
            "b1",
            "r1",
            "测试规则",
            "move".into(),
            "a".into(),
            Some("b".into()),
            None,
        );
        append_records(&log, &[r]).unwrap();
        let all = read_all(&log);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].batch_id, "b1");
        assert!(!all[0].undone);
        let _ = fs::remove_file(&log);
    }

    #[test]
    fn undo_moves_file_back() {
        let dir = std::env::temp_dir().join("filepilot_oplog_undo");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.pdf");
        let dest_dir = dir.join("out");
        fs::create_dir_all(&dest_dir).unwrap();
        fs::write(&src, "data").unwrap();

        // 模拟一次移动
        let moved = dest_dir.join("a.pdf");
        fs::rename(&src, &moved).unwrap();

        let log = tmp_log("undo");
        let r = OpRecord::new(
            "b1",
            "r1",
            "测试",
            "move".into(),
            src.to_string_lossy().to_string(),
            Some(moved.to_string_lossy().to_string()),
            None,
        );
        append_records(&log, &[r]).unwrap();

        let undone = undo_last_batch(&log).unwrap();
        assert_eq!(undone.len(), 1);
        assert!(src.exists(), "文件应被移回原位置");
        assert!(!moved.exists());

        // 撤销后标记为 undone，再次撤销应返回 0
        assert!(undo_last_batch(&log).unwrap().is_empty());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&log);
    }

    #[test]
    fn undo_copy_deletes_duplicate() {
        let dir = std::env::temp_dir().join("filepilot_oplog_undocopy");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.pdf");
        let copied = dir.join("a_copy.pdf");
        fs::write(&src, "data").unwrap();
        fs::copy(&src, &copied).unwrap();

        let log = tmp_log("undocopy");
        let r = OpRecord::new(
            "b1",
            "r1",
            "测试",
            "copy".into(),
            src.to_string_lossy().to_string(),
            Some(copied.to_string_lossy().to_string()),
            None,
        );
        append_records(&log, &[r]).unwrap();

        let undone = undo_last_batch(&log).unwrap();
        assert_eq!(undone.len(), 1);
        assert!(src.exists(), "原文件应保留");
        assert!(!copied.exists(), "副本应被删除");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&log);
    }

    #[test]
    fn undo_recycle_restores_from_trash() {
        let dir = std::env::temp_dir().join("filepilot_oplog_undorecycle");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("setup.exe");
        fs::write(&src, "data").unwrap();

        // 模拟一次移入回收站
        trash::delete(&src).expect("测试环境需要回收站支持");
        assert!(!src.exists(), "文件应已进入回收站");

        let log = tmp_log("undorecycle");
        let r = OpRecord::new(
            "b1",
            "r1",
            "测试",
            "recycle".into(),
            src.to_string_lossy().to_string(),
            None,
            None,
        );
        append_records(&log, &[r]).unwrap();

        let undone = undo_last_batch(&log).unwrap();
        assert_eq!(undone.len(), 1);
        assert!(src.exists(), "文件应从回收站恢复到原位置");

        // 撤销后标记为 undone，再次撤销应返回 0
        assert!(undo_last_batch(&log).unwrap().is_empty());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&log);
    }

    #[test]
    #[cfg(windows)]
    fn recycle_undo_preserves_shell_hidden_extension() {
        let dir = std::env::temp_dir().join(format!(
            "filepilot_recycle_display_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        // Shell hides .lnk even when Explorer is configured to show extensions.
        // Exercise Shell's long-path metadata against a short-path source even
        // when the test process itself has a long TEMP path.
        let src = short_path(&dir).join("filepilot-regression.lnk");
        fs::write(&src, "test payload; never executed").unwrap();
        let id = trash_delete_capturing_id(src.to_str().unwrap()).unwrap();
        let mut items = trash::os_limited::list().unwrap();
        let item = items
            .iter_mut()
            .find(|item| Some(item.id.to_string_lossy().as_ref()) == id.as_deref())
            .unwrap();
        item.name = "filepilot-regression".into();
        // Explicitly simulate a hidden display name even on this machine's settings.
        accurate_recycle_names(&mut items).unwrap();
        let original_path = items
            .iter()
            .find(|item| Some(item.id.to_string_lossy().as_ref()) == id.as_deref())
            .unwrap()
            .original_path();
        // The parent can use a long or 8.3 name; the full filename must be exact.
        assert_eq!(original_path.file_name(), src.file_name());
        assert!(
            paths_equal(&original_path, &src),
            "回收站原路径应指向来源文件：{original_path:?} / {src:?}"
        );
        let record = OpRecord::new(
            "b",
            "r",
            "测试",
            "recycle".into(),
            src.to_string_lossy().to_string(),
            None,
            id,
        );
        restore_from_trash(&record).unwrap();
        assert!(
            src.exists(),
            "应保留原始扩展名，恢复目录现有内容：{:?}",
            fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fs::read_to_string(&src).unwrap(),
            "test payload; never executed"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn recycle_undo_missing_item_reports_failure_without_marking_undone() {
        let log = tmp_log(&format!("missing_recycle_{}", uuid::Uuid::new_v4()));
        let src =
            std::env::temp_dir().join(format!("filepilot_missing_{}.exe", uuid::Uuid::new_v4()));
        let record = OpRecord::new(
            "b",
            "r",
            "测试",
            "recycle".into(),
            src.to_string_lossy().to_string(),
            None,
            None,
        );
        append_records(&log, &[record]).unwrap();
        assert!(undo_last_batch(&log)
            .unwrap_err()
            .to_string()
            .contains("回收站中找不到"));
        assert!(!read_all(&log)[0].undone);
        fs::remove_file(log).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn recycle_undo_legacy_record_preserves_hidden_extension() {
        let dir =
            std::env::temp_dir().join(format!("filepilot_recycle_legacy_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("filepilot-regression.lnk");
        fs::write(&src, "legacy test payload; never executed").unwrap();
        trash::delete(&src).unwrap();
        let record = OpRecord::new(
            "b",
            "r",
            "测试",
            "recycle".into(),
            src.to_string_lossy().to_string(),
            None,
            None,
        );
        restore_from_trash(&record).unwrap();
        assert_eq!(
            fs::read_to_string(&src).unwrap(),
            "legacy test payload; never executed"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recycle_undo_via_captured_trash_id() {
        let dir = std::env::temp_dir().join("filepilot_oplog_recycle_id");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("app-setup.exe");
        fs::write(&src, "data").unwrap();

        // 走 apply_plan 执行回收站动作（应捕获 trash id）
        let item = crate::engine::PlanItem {
            src: src.to_string_lossy().to_string(),
            action_desc: "移入回收站".into(),
            dest: None,
            kind: "recycle".into(),
            rule_name: "测试".into(),
            is_directory: false,
            source_digest: None,
            error: None,
            archive: None,
        };
        let log = tmp_log("recycle_id");
        let records = crate::engine::apply_plan(&[item], "b1", "r1");
        assert_eq!(records.len(), 1);
        assert!(!src.exists(), "文件应已进入回收站");
        #[cfg(any(
            target_os = "windows",
            all(
                unix,
                not(target_os = "macos"),
                not(target_os = "ios"),
                not(target_os = "android")
            )
        ))]
        assert!(records[0].trash_id.is_some(), "应捕获到回收站条目 id");
        append_records(&log, &records).unwrap();

        let undone = undo_last_batch(&log).unwrap();
        assert_eq!(undone.len(), 1);
        assert!(src.exists(), "文件应从回收站恢复到原位置");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&log);
    }
}
