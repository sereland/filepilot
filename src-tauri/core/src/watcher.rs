//! 文件夹监控：用 notify-debouncer-mini 监听，防抖后触发规则自动执行。
//!
//! 设计：
//! - 每个被规则引用的 watch_folder 起一个 debouncer（2 秒防抖）。
//! - 防抖事件到达后，对覆盖该文件夹的**已启用**规则执行 apply（非 dry-run，
//!   因为规则在保存时已经过用户的 dry-run 确认）。
//! - 执行后通过 callback 通知上层（上层负责写 oplog、刷新 UI）。
//! - 监控线程与主线程通过 mpsc channel 通信，watcher 可整体启停。

use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crate::engine;
use crate::rules::Rule;

/// 防抖时长：文件夹"安静"这么久后才触发整理
const DEBOUNCE_SECS: u64 = 2;

pub enum WatchEvent {
    /// 某个被监控文件夹有稳定变化，需要跑规则
    FolderSettled(PathBuf),
    /// 停止监控线程
    Shutdown,
}

pub struct WatchHandle {
    tx: Sender<WatchEvent>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WatchHandle {
    pub fn shutdown(mut self) {
        let _ = self.tx.send(WatchEvent::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// 回调：对给定文件夹执行匹配的规则。由调用方提供（需要访问 AppState）。
pub type ApplyCallback = Box<dyn Fn(&Path) -> usize + Send + 'static>;

/// 启动监控线程，监听 folders。返回 handle，drop/shutdown 可停止。
pub fn start_watching(folders: Vec<PathBuf>, on_settled: ApplyCallback) -> WatchHandle {
    let (tx, rx): (Sender<WatchEvent>, Receiver<WatchEvent>) = mpsc::channel();
    let tx_clone = tx.clone();

    let thread = std::thread::spawn(move || {
        // 每个文件夹一个 debouncer（去重）
        let mut debouncers = HashMap::new();
        for folder in &folders {
            let tx2 = tx_clone.clone();
            let folder2 = folder.clone();
            let handler = move |res: DebounceEventResult| {
                if res.is_ok() {
                    let _ = tx2.send(WatchEvent::FolderSettled(folder2.clone()));
                }
            };
            match new_debouncer(Duration::from_secs(DEBOUNCE_SECS), handler) {
                Ok(mut d) => {
                    if d
                        .watcher()
                        .watch(folder, notify::RecursiveMode::NonRecursive)
                        .is_ok()
                    {
                        debouncers.insert(folder.clone(), d);
                    }
                }
                Err(_) => continue,
            }
        }
        // debouncers 必须活着，绑定到 _keepalive
        let _keepalive = debouncers;

        // 事件循环
        loop {
            match rx.recv() {
                Ok(WatchEvent::FolderSettled(folder)) => {
                    on_settled(&folder);
                }
                Ok(WatchEvent::Shutdown) | Err(_) => break,
            }
        }
    });

    WatchHandle {
        tx,
        thread: Some(thread),
    }
}

/// 对单个文件夹跑所有启用的相关规则，返回执行的操作数。
/// engine 层函数，供 watcher 回调和 Tauri command 共用。
pub fn apply_rules_for_folder(rules: &[Rule], folder: &Path) -> Vec<(String, Vec<crate::oplog::OpRecord>)> {
    let mut out = Vec::new();
    for rule in rules.iter().filter(|r| r.enabled) {
        let watches = rule
            .watch_folders
            .iter()
            .any(|f| Path::new(f) == folder);
        if !watches {
            continue;
        }
        let plan = engine::dry_run(rule, &[folder.to_path_buf()]);
        if plan.is_empty() {
            continue;
        }
        let batch_id = uuid::Uuid::new_v4().to_string();
        let records = engine::apply_plan(&plan, &batch_id, &rule.id);
        if !records.is_empty() {
            out.push((rule.id.clone(), records));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Action, Condition};

    #[test]
    fn applies_matching_enabled_rules_only() {
        let dir = std::env::temp_dir().join("filepilot_watcher_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 让文件静默（修改时间 10 秒前）
        let f1 = dir.join("a.pdf");
        std::fs::write(&f1, "x").unwrap();
        set_old(&f1);
        let f2 = dir.join("b.txt");
        std::fs::write(&f2, "x").unwrap();
        set_old(&f2);

        let dest = dir.join("docs");
        let rule_on = Rule {
            id: "on".into(),
            name: "开".into(),
            enabled: true,
            watch_folders: vec![dir.to_string_lossy().to_string()],
            conditions: vec![Condition::Extension {
                exts: vec!["pdf".into()],
            }],
            actions: vec![Action::Move {
                dest: dest.to_string_lossy().to_string(),
            }],
        };
        let rule_off = Rule {
            id: "off".into(),
            name: "关".into(),
            enabled: false,
            ..rule_on.clone()
        };

        let res = apply_rules_for_folder(&[rule_on, rule_off], &dir);
        assert_eq!(res.len(), 1, "只有启用的规则应执行");
        assert_eq!(res[0].0, "on");
        assert!(dest.join("a.pdf").exists());
        assert!(dir.join("b.txt").exists(), "不匹配的文件不动");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn set_old(p: &Path) {
        let f = std::fs::File::options().write(true).open(p).unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(10);
        f.set_modified(t).unwrap();
    }
}
