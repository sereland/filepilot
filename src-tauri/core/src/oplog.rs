//! 操作日志：每次执行以 JSONL 追加记录，支持按批次撤销。
//!
//! 撤销语义：
//! - move/rename：把文件移回原路径（原位置被占用则自动加编号，绝不覆盖）
//! - copy：删除复制出的副本
//! - recycle：无法撤销（已进系统回收站，用户可手动恢复），标记为不可撤销

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
    /// move | copy | rename | recycle
    pub kind: String,
    pub src: String,
    /// 执行后的实际目标路径（重名加编号后的最终路径）
    pub dest: Option<String>,
    pub undone: bool,
    /// 回收站条目 id（仅 recycle，用于撤销时精确恢复）；旧记录为 None
    #[serde(default)]
    pub trash_id: Option<String>,
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
            trash_id,
        }
    }
}

/// 追加一批记录到 JSONL 日志
pub fn append_records(log_path: &Path, records: &[OpRecord]) -> std::io::Result<()> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(log_path)?;
    for r in records {
        writeln!(f, "{}", serde_json::to_string(r).unwrap())?;
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
    all
}

/// 每条规则最近一次实际整理时间，不受操作记录页条数限制影响。
/// 已撤销的操作仍算执行过，撤销不会把规则变回“暂未执行”。
pub fn rule_last_runs(log_path: &Path) -> std::collections::HashMap<String, String> {
    let mut last_runs = std::collections::HashMap::new();
    for record in read_all(log_path) {
        last_runs.insert(record.rule_id, record.timestamp);
    }
    last_runs
}

/// 撤销最近一个未撤销的批次，返回被撤销的记录。
/// 批次内按逆序撤销（后执行的先回滚）。
pub fn undo_last_batch(log_path: &Path) -> std::io::Result<Vec<OpRecord>> {
    let mut all = read_all(log_path);
    // 找到最近的未撤销批次
    let target_batch = all
        .iter()
        .rev()
        .find(|r| !r.undone)
        .map(|r| r.batch_id.clone());
    let batch_id = match target_batch {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    let mut undone = Vec::new();
    for record in all.iter_mut().rev() {
        if record.batch_id != batch_id || record.undone {
            continue;
        }
        if undo_one(record).is_ok() {
            record.undone = true;
            undone.push(record.clone());
        }
    }

    // 写回整份日志（P0 数据量小，全量重写可接受）
    rewrite_all(log_path, &all)?;
    Ok(undone)
}

fn undo_one(record: &OpRecord) -> std::io::Result<()> {
    match record.kind.as_str() {
        "move" | "rename" => {
            let dest = Path::new(record.dest.as_ref().ok_or_else(|| {
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
    trash::delete(src).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let after = trash::os_limited::list()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(after
        .into_iter()
        .find(|i| !before.contains(&i.id))
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
    trash::delete(src).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
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
    let items = trash::os_limited::list()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    // 1) 按 trash id 精确恢复
    if let Some(tid) = record.trash_id.as_deref() {
        if let Some(item) = items.iter().find(|i| i.id.to_string_lossy() == tid) {
            return trash::os_limited::restore_all(std::iter::once(item.clone()))
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
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
            format!(
                "在回收站中找不到 {}，请手动从系统回收站恢复",
                record.src
            ),
        ));
    }
    // 逐个恢复：同名双胞胎一次只恢复一个，避免 RestoreTwins 错误
    let mut last_err = None;
    while let Some(item) = candidates.pop() {
        match trash::os_limited::restore_all(std::iter::once(item)) {
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

/// 路径归一化后比较：去 verbatim 前缀、统一分隔符、Windows 下不区分大小写
pub fn normalize_path(p: &Path) -> String {
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
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;
    for r in records {
        writeln!(f, "{}", serde_json::to_string(r).unwrap())?;
    }
    Ok(())
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

    #[test]
    fn rule_last_runs_includes_old_and_undone_records() {
        let log = tmp_log("rule_last_runs");
        let mut old = OpRecord::new("old", "older-rule", "旧规则", "copy".into(), "a".into(), Some("b".into()), None);
        old.timestamp = "2026-01-01T10:00:00+08:00".into();
        old.undone = true;
        let mut records = vec![old];
        for index in 0..120 {
            let mut record = OpRecord::new("new", "recent-rule", "新规则", "copy".into(), "a".into(), Some("b".into()), None);
            record.timestamp = format!("time-{}", index);
            records.push(record);
        }
        append_records(&log, &records).unwrap();
        assert!(read_recent(&log, 100).iter().all(|r| r.rule_id == "recent-rule"));
        let last = rule_last_runs(&log);
        assert_eq!(last.get("older-rule").unwrap(), "2026-01-01T10:00:00+08:00");
        assert_eq!(last.get("recent-rule").unwrap(), "time-119");
        assert!(!last.contains_key("never-run"));
        let _ = fs::remove_file(&log);
    }

    #[test]
    fn append_and_read_roundtrip() {
        let log = tmp_log("rt");
        let r = OpRecord::new("b1", "r1", "测试规则", "move".into(), "a".into(), Some("b".into()), None);
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
        assert!(
            records[0].trash_id.is_some(),
            "应捕获到回收站条目 id"
        );
        append_records(&log, &records).unwrap();

        let undone = undo_last_batch(&log).unwrap();
        assert_eq!(undone.len(), 1);
        assert!(src.exists(), "文件应从回收站恢复到原位置");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&log);
    }
}
