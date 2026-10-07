//! 规则持久化：rules.json 存于应用数据目录。

use crate::rules::Rule;
use std::path::{Path, PathBuf};

pub struct RuleStore {
    path: PathBuf,
    rules: Vec<Rule>,
}

impl RuleStore {
    pub fn load(path: &Path) -> Self {
        let rules = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            rules,
        }
    }

    pub fn all(&self) -> Vec<Rule> {
        self.rules.clone()
    }

    pub fn get(&self, id: &str) -> Option<Rule> {
        self.rules.iter().find(|r| r.id == id).cloned()
    }

    /// 新增或更新（id 为空则分配新 id）
    pub fn upsert(&mut self, mut rule: Rule) -> Rule {
        if rule.id.is_empty() {
            rule.id = uuid::Uuid::new_v4().to_string();
        }
        match self.rules.iter().position(|r| r.id == rule.id) {
            Some(i) => self.rules[i] = rule.clone(),
            None => self.rules.push(rule.clone()),
        }
        let _ = self.save();
        rule
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        let changed = self.rules.len() != before;
        if changed {
            let _ = self.save();
        }
        changed
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        let mut changed = false;
        for r in self.rules.iter_mut() {
            if r.id == id {
                r.enabled = enabled;
                changed = true;
            }
        }
        if changed {
            let _ = self.save();
        }
        changed
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.rules).unwrap();
        std::fs::write(&self.path, json)
    }
}
