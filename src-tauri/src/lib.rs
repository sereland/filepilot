//! FilePilot 主入口：应用状态 + Tauri commands。
//!
//! 核心逻辑在 filepilot-core crate（可独立测试），这里只做胶水层。

use filepilot_core::{engine, oplog, rules, store, watcher};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use store::RuleStore;
use tauri::{AppHandle, Emitter, Manager, State};
use watcher::WatchHandle;

struct AppState {
    store: Mutex<RuleStore>,
    oplog_path: PathBuf,
    watcher: Mutex<Option<WatchHandle>>,
}

impl AppState {
    fn new(rules_path: PathBuf, oplog_path: PathBuf) -> Self {
        Self {
            store: Mutex::new(RuleStore::load(&rules_path)),
            oplog_path,
            watcher: Mutex::new(None),
        }
    }
}

// ---------- 规则管理 ----------

#[tauri::command]
fn get_rules(state: State<'_, AppState>) -> Vec<Rule> {
    state.store.lock().unwrap().all()
}

#[tauri::command]
fn save_rule(state: State<'_, AppState>, rule: Rule) -> Rule {
    state.store.lock().unwrap().upsert(rule)
}

#[tauri::command]
fn delete_rule(state: State<'_, AppState>, id: String) -> bool {
    state.store.lock().unwrap().delete(&id)
}

#[tauri::command]
fn set_rule_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> bool {
    state.store.lock().unwrap().set_enabled(&id, enabled)
}

// ---------- 执行 ----------

#[tauri::command]
fn preview_rule(state: State<'_, AppState>, id: String) -> Vec<engine::PlanItem> {
    let store = state.store.lock().unwrap();
    let rule = match store.get(&id) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let folders: Vec<PathBuf> = rule.watch_folders.iter().map(PathBuf::from).collect();
    engine::dry_run(&rule, &folders)
}

#[tauri::command]
fn apply_rule_now(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<Vec<OpRecord>, String> {
    let store = state.store.lock().unwrap();
    let rule = store.get(&id).ok_or_else(|| "规则不存在".to_string())?;
    drop(store);

    let folders: Vec<PathBuf> = rule.watch_folders.iter().map(PathBuf::from).collect();
    let plan = engine::dry_run(&rule, &folders);
    if plan.is_empty() {
        return Ok(Vec::new());
    }
    let batch_id = uuid::Uuid::new_v4().to_string();
    let records = engine::apply_plan(&plan, &batch_id, &rule.id);
    if !records.is_empty() {
        oplog::append_records(&state.oplog_path, &records)
            .map_err(|e| format!("写入日志失败: {}", e))?;
        let _ = app.emit("fp://oplog-updated", ());
    }
    Ok(records)
}

#[tauri::command]
fn get_oplog(state: State<'_, AppState>, limit: usize) -> Vec<OpRecord> {
    oplog::read_recent(&state.oplog_path, limit.min(500))
}

#[tauri::command]
fn undo_last(state: State<'_, AppState>, app: AppHandle) -> Result<usize, String> {
    let n = oplog::undo_last_batch(&state.oplog_path).map_err(|e| e.to_string())?;
    if n > 0 {
        let _ = app.emit("fp://oplog-updated", ());
    }
    Ok(n)
}

// ---------- 监控 ----------

/// 收集所有启用规则引用的文件夹（去重）
fn watched_folders(state: &AppState) -> Vec<PathBuf> {
    let store = state.store.lock().unwrap();
    let mut set = HashSet::new();
    for rule in store.all().iter().filter(|r| r.enabled) {
        for f in &rule.watch_folders {
            let p = PathBuf::from(f);
            if p.is_dir() {
                set.insert(p);
            }
        }
    }
    set.into_iter().collect()
}

#[tauri::command]
fn start_monitoring(state: State<'_, AppState>, app: AppHandle) -> bool {
    // 先停掉旧的
    stop_monitoring_inner(state.inner());

    let folders = watched_folders(state.inner());
    if folders.is_empty() {
        return false;
    }

    let app2 = app.clone();
    let oplog_path = state.oplog_path.clone();
    let callback: watcher::ApplyCallback = Box::new(move |folder| {
        let app_state: &AppState = app2.state();
        let rules = app_state.store.lock().unwrap().all();
        let results = watcher::apply_rules_for_folder(&rules, folder);
        let mut total = 0;
        for (_rule_id, records) in &results {
            total += records.len();
            let _ = oplog::append_records(&oplog_path, records);
        }
        if total > 0 {
            let _ = app2.emit("fp://oplog-updated", total);
        }
        total
    });

    let handle = watcher::start_watching(folders, callback);
    *state.watcher.lock().unwrap() = Some(handle);
    true
}

fn stop_monitoring_inner(state: &AppState) {
    if let Some(handle) = state.watcher.lock().unwrap().take() {
        handle.shutdown();
    }
}

#[tauri::command]
fn stop_monitoring(state: State<'_, AppState>) {
    stop_monitoring_inner(state.inner());
}

// ---------- 对话框 ----------

#[tauri::command]
fn pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|p| p.to_string())
}

// ---------- 入口 ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_local_data_dir()
                .expect("无法获取应用数据目录");
            let _ = std::fs::create_dir_all(&data_dir);
            let state = AppState::new(
                data_dir.join("rules.json"),
                data_dir.join("oplog.jsonl"),
            );
            app.manage(state);

            // 启动时自动开始监控
            let handle = app.handle().clone();
            let state_ref: tauri::State<'_, AppState> = handle.state();
            start_monitoring(state_ref, handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_rules,
            save_rule,
            delete_rule,
            set_rule_enabled,
            preview_rule,
            apply_rule_now,
            get_oplog,
            undo_last,
            start_monitoring,
            stop_monitoring,
            pick_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
