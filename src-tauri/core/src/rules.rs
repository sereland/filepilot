//! 规则数据模型：条件、动作、规则，以及文件匹配逻辑。
//!
//! 保留旧条件与动作格式；条件组编译为 JSONLogic，文件动作可连续执行。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 匹配条件
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// 扩展名匹配，如 ["pdf", "docx"]（不区分大小写，不带点）
    Extension {
        exts: Vec<String>,
    },
    /// 文件名包含关键词（支持中文，不区分大小写）
    NameContains {
        keyword: String,
    },
    /// 文件大小大于 N 字节
    SizeGreaterThan {
        bytes: u64,
    },
    /// N 天内创建的文件
    CreatedWithinDays {
        days: u64,
    },
    NameMatch {
        operator: NameOperator,
        keyword: String,
    },
    SizeLessThan {
        bytes: u64,
    },
    SizeBetween {
        min: u64,
        max: u64,
    },
    DateMatch {
        field: FileDate,
        operator: DateOperator,
        days: u64,
    },
    Group {
        mode: MatchMode,
        #[serde(default)]
        negate: bool,
        conditions: Vec<Condition>,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NameOperator {
    Contains,
    NotContains,
    Equals,
    StartsWith,
    EndsWith,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileDate {
    Created,
    Modified,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DateOperator {
    WithinDays,
    OlderThanDays,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    All,
    Any,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    Now,
    Created,
    Modified,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileOperation {
    Move,
    Copy,
}

/// 整理动作
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveRetention {
    #[default]
    All,
    Latest,
}

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
    MoveAndExtract {
        dest: String,
        #[serde(default)]
        retention: ArchiveRetention,
    },
    Organize {
        operation: FileOperation,
        dest: String,
        subfolder: String,
        date_source: DateSource,
    },
    RenameWithDate {
        pattern: String,
        date_source: DateSource,
    },
}

/// 一条整理规则
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    #[default]
    File,
    Directory,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub target_kind: TargetKind,
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
            target_kind: TargetKind::File,
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
    pub is_directory: bool,
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
            is_directory: md.is_dir(),
            ext: if md.is_dir() { String::new() } else { ext },
            size: md.len(),
            created: md.created().ok(),
            modified: md.modified().ok(),
        })
    }
}

impl Condition {
    pub fn matches(&self, meta: &FileMeta) -> bool {
        crate::matching::Matcher::new(std::slice::from_ref(self))
            .map(|matcher| matcher.matches(meta))
            .unwrap_or(false)
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
            Condition::NameMatch { operator, keyword } => format!(
                "名称{}「{}」",
                match operator {
                    NameOperator::Contains => "包含",
                    NameOperator::NotContains => "不包含",
                    NameOperator::Equals => "等于",
                    NameOperator::StartsWith => "开头是",
                    NameOperator::EndsWith => "结尾是",
                },
                keyword
            ),
            Condition::SizeLessThan { bytes } => format!("文件小于 {}", format_bytes(*bytes)),
            Condition::SizeBetween { min, max } => {
                format!("文件大小 {} 至 {}", format_bytes(*min), format_bytes(*max))
            }
            Condition::DateMatch {
                field,
                operator,
                days,
            } => format!(
                "{}{} {} 天",
                if *field == FileDate::Created {
                    "创建时间"
                } else {
                    "修改时间"
                },
                if *operator == DateOperator::WithinDays {
                    "在最近"
                } else {
                    "距今至少"
                },
                days
            ),
            Condition::Group {
                mode,
                negate,
                conditions,
            } => format!(
                "{}（{}）",
                if *negate { "排除" } else { "" },
                conditions
                    .iter()
                    .map(Condition::describe)
                    .collect::<Vec<_>>()
                    .join(if *mode == MatchMode::All {
                        " 且 "
                    } else {
                        " 或 "
                    })
            ),
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
            Action::MoveAndExtract { dest, retention } => format!(
                "移动并解压到 {}（{}）",
                dest,
                if *retention == ArchiveRetention::Latest {
                    "只保留最新一份"
                } else {
                    "保留全部"
                }
            ),
            Action::Organize {
                operation,
                dest,
                subfolder,
                ..
            } => format!(
                "{} {}/{}",
                if *operation == FileOperation::Move {
                    "移动到"
                } else {
                    "复制到"
                },
                dest,
                subfolder
            ),
            Action::RenameWithDate { pattern, .. } => format!("重命名为 {}", pattern),
        }
    }
}

impl Rule {
    pub fn compile_matcher(&self) -> Result<crate::matching::Matcher, String> {
        fn directory_condition(c: &Condition) -> bool {
            match c {
                Condition::Group { conditions, .. } => conditions.iter().all(directory_condition),
                Condition::Extension { .. }
                | Condition::SizeGreaterThan { .. }
                | Condition::SizeLessThan { .. }
                | Condition::SizeBetween { .. } => false,
                _ => true,
            }
        }
        if self.target_kind == TargetKind::Directory
            && !self.conditions.iter().all(directory_condition)
        {
            return Err("文件夹不支持扩展名和文件大小条件。".into());
        }
        crate::matching::Matcher::new(&self.conditions)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.watch_folders.is_empty()
            || self
                .watch_folders
                .iter()
                .any(|folder| folder.trim().is_empty())
        {
            return Err("请填写规则名称和来源文件夹。".into());
        }
        self.compile_matcher()?;
        if self.actions.is_empty() || self.actions.len() > 20 {
            return Err("请设置 1 至 20 个执行动作。".into());
        }
        if self.target_kind == TargetKind::Directory && self.actions.len() != 1 {
            return Err("文件夹整理目前每条规则支持一个动作。".into());
        }
        for (index, action) in self.actions.iter().enumerate() {
            match action {
                Action::Move { dest } | Action::Copy { dest } | Action::Organize { dest, .. }
                    if dest.trim().is_empty() =>
                {
                    return Err("请选择目标文件夹。".into())
                }
                Action::MoveAndExtract { dest, .. }
                    if dest.trim().is_empty()
                        || self.actions.len() != 1
                        || self.target_kind == TargetKind::Directory =>
                {
                    return Err("移动并解压仅适用于文件，须独立使用并设置目标。".into())
                }
                Action::Rename { pattern } | Action::RenameWithDate { pattern, .. }
                    if pattern.trim().is_empty() =>
                {
                    return Err("请输入重命名格式。".into())
                }
                Action::MoveToRecycleBin if index + 1 != self.actions.len() => {
                    return Err("移入回收站须作为最后一个动作。".into())
                }
                _ => (),
            }
            let sample = FileMeta {
                path: PathBuf::from("example.pdf"),
                name: "example.pdf".into(),
                is_directory: self.target_kind == TargetKind::Directory,
                ext: if self.target_kind == TargetKind::Directory {
                    String::new()
                } else {
                    "pdf".into()
                },
                size: 1,
                created: Some(SystemTime::now()),
                modified: Some(SystemTime::now()),
            };
            if let Action::Rename { pattern } = crate::templates::resolve(action, &sample)? {
                if !crate::archive::valid_component(&pattern) {
                    return Err("重命名格式不能包含路径或非法名称。".into());
                }
            }
        }
        Ok(())
    }
    pub fn matches(&self, meta: &FileMeta) -> bool {
        (self.target_kind == TargetKind::Directory) == meta.is_directory
            && self
                .compile_matcher()
                .map(|matcher| matcher.matches(meta))
                .unwrap_or(false)
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
            is_directory: false,
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
        let recent = meta(
            "a.txt",
            10,
            Some(SystemTime::now() - Duration::from_secs(3600)),
        );
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
