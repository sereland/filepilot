//! 规则数据模型：条件、动作、规则，以及文件匹配逻辑。
//!
//! P0 支持 4 种条件 × 4 种动作，条件之间为 AND 语义。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 匹配条件
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// 扩展名匹配，如 ["pdf", "docx"]（不区分大小写，不带点）
    Extension { exts: Vec<String> },
    /// 文件名包含关键词（支持中文，不区分大小写）
    NameContains { keyword: String },
    /// 文件大小大于 N 字节
    SizeGreaterThan { bytes: u64 },
    /// N 天内创建的文件
    CreatedWithinDays { days: u64 },
}

/// 整理动作
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// 移动到目标文件夹（自动创建）
    Move { dest: String },
    /// 复制到目标文件夹
    Copy { dest: String },
    /// 按模板重命名，支持变量 {name} {ext} {date} {datetime}
    Rename { pattern: String },
    /// 移入回收站（安全删除）
    MoveToRecycleBin,
}

/// 一条整理规则
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// 监控的文件夹（字符串路径）
    pub watch_folders: Vec<String>,
    /// 条件之间为 AND：全部满足才触发
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            watch_folders: Vec::new(),
            conditions: Vec::new(),
            actions: Vec::new(),
        }
    }
}

/// 从文件系统读取的文件元信息
#[derive(Clone, Debug)]
pub struct FileMeta {
    pub path: PathBuf,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub created: Option<SystemTime>,
    /// 最后修改时间（用于跳过正在写入的文件）
    pub modified: Option<SystemTime>,
}

impl FileMeta {
    pub fn from_path(path: &Path) -> std::io::Result<Self> {
        let md = std::fs::metadata(path)?;
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Ok(Self {
            path: path.to_path_buf(),
            name,
            ext,
            size: md.len(),
            created: md.created().ok(),
            modified: md.modified().ok(),
        })
    }
}

impl Condition {
    pub fn matches(&self, meta: &FileMeta) -> bool {
        match self {
            Condition::Extension { exts } => exts
                .iter()
                .any(|e| e.trim_start_matches('.').to_lowercase() == meta.ext),
            Condition::NameContains { keyword } => {
                meta.name.to_lowercase().contains(&keyword.to_lowercase())
            }
            Condition::SizeGreaterThan { bytes } => meta.size > *bytes,
            Condition::CreatedWithinDays { days } => match meta.created {
                Some(t) => t
                    .elapsed()
                    .map(|d| d.as_secs() < days * 86400)
                    .unwrap_or(false),
                None => false,
            },
        }
    }

    /// 人类可读的中文描述（给 UI 用）
    pub fn describe(&self) -> String {
        match self {
            Condition::Extension { exts } => format!("扩展名为 {}", exts.join("、")),
            Condition::NameContains { keyword } => format!("文件名包含「{}」", keyword),
            Condition::SizeGreaterThan { bytes } => {
                format!("文件大于 {}", format_bytes(*bytes))
            }
            Condition::CreatedWithinDays { days } => format!("{} 天内创建", days),
        }
    }
}

impl Action {
    /// 人类可读的中文描述（给 UI 用）
    pub fn describe(&self) -> String {
        match self {
            Action::Move { dest } => format!("移动到 {}", dest),
            Action::Copy { dest } => format!("复制到 {}", dest),
            Action::Rename { pattern } => format!("重命名为 {}", pattern),
            Action::MoveToRecycleBin => "移入回收站".to_string(),
        }
    }
}

impl Rule {
    pub fn matches(&self, meta: &FileMeta) -> bool {
        !self.conditions.is_empty()
            && self.conditions.iter().all(|c| c.matches(meta))
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} {}", bytes, UNITS[u])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn meta(name: &str, size: u64, created: Option<SystemTime>) -> FileMeta {
        let path = PathBuf::from(format!("/tmp/{}", name));
        let ext = Path::new(name)
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        FileMeta {
            path,
            name: name.to_string(),
            ext,
            size,
            created,
            modified: None,
        }
    }

    #[test]
    fn extension_matches_case_insensitive() {
        let m = meta("报告.PDF", 100, None);
        let c = Condition::Extension {
            exts: vec!["pdf".into()],
        };
        assert!(c.matches(&m));
    }

    #[test]
    fn name_contains_chinese() {
        let m = meta("2024年度发票.pdf", 100, None);
        let c = Condition::NameContains {
            keyword: "发票".into(),
        };
        assert!(c.matches(&m));
    }

    #[test]
    fn size_condition() {
        let m = meta("big.zip", 200_000_000, None);
        assert!(Condition::SizeGreaterThan { bytes: 100_000_000 }.matches(&m));
        assert!(!Condition::SizeGreaterThan { bytes: 500_000_000 }.matches(&m));
    }

    #[test]
    fn created_within_days() {
        let recent = meta("a.txt", 10, Some(SystemTime::now() - Duration::from_secs(3600)));
        let old = meta(
            "b.txt",
            10,
            Some(SystemTime::now() - Duration::from_secs(10 * 86400)),
        );
        let c = Condition::CreatedWithinDays { days: 7 };
        assert!(c.matches(&recent));
        assert!(!c.matches(&old));
        // 没有创建时间信息时不匹配（宁可漏判，不错判）
        assert!(!c.matches(&meta("c.txt", 10, None)));
    }

    #[test]
    fn rule_requires_all_conditions() {
        let rule = Rule {
            conditions: vec![
                Condition::Extension {
                    exts: vec!["pdf".into()],
                },
                Condition::NameContains {
                    keyword: "发票".into(),
                },
            ],
            ..Default::default()
        };
        assert!(rule.matches(&meta("发票2024.pdf", 10, None)));
        assert!(!rule.matches(&meta("合同2024.pdf", 10, None)));
        assert!(!rule.matches(&meta("发票2024.docx", 10, None)));
    }

    #[test]
    fn rule_with_no_conditions_matches_nothing() {
        let rule = Rule::default();
        assert!(!rule.matches(&meta("a.pdf", 10, None)));
    }
}
