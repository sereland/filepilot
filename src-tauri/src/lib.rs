//! FilePilot 主入口：应用状态 + Tauri commands。
//!
//! 核心逻辑在 filepilot-core crate（可独立测试），这里只做胶水层。

use filepilot_core::{engine, oplog, rules, store, watcher};
use oplog::OpRecord;
use rules::Rule;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use store::RuleStore;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

struct AppState {
    store: Mutex<RuleStore>,
    oplog_path: PathBuf,
    watcher: Mutex<Option<watcher::WatchHandle>>,
    monitoring: Mutex<bool>,
    /// 撤销保护名单：(文件路径, 撤销时间)，持久化到磁盘。
    /// 监控扫描时跳过"撤销后未被修改过"的文件，避免撤销→恢复→被规则再次处理→无限循环。
    recently_undone: Mutex<Vec<(PathBuf, SystemTime)>>,
    skip_list_path: PathBuf,
}

impl AppState {
    fn new(rules_path: PathBuf, oplog_path: PathBuf, skip_list_path: PathBuf) -> Self {
        Self {
            store: Mutex::new(RuleStore::load(&rules_path)),
            oplog_path,
            watcher: Mutex::new(None),
            monitoring: Mutex::new(false),
            recently_undone: Mutex::new(load_skip_list(&skip_list_path)),
            skip_list_path,
        }
    }
}

/// 从磁盘加载撤销保护名单（JSON：[(路径, 撤销时间的 unix 秒)]），损坏或缺失时返回空
fn load_skip_list(path: &Path) -> Vec<(PathBuf, SystemTime)> {
    let data = std::fs::read_to_string(path).unwrap_or_default();
    if data.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<(String, u64)>>(data.as_str())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(p, secs)| {
            SystemTime::UNIX_EPOCH
                .checked_add(std::time::Duration::from_secs(secs))
                .map(|t| (PathBuf::from(p), t))
        })
        .collect()
}

/// 持久化撤销保护名单，失败时静默忽略（下次撤销会重试）
fn save_skip_list(path: &Path, list: &[(PathBuf, SystemTime)]) {
    let data: Vec<(String, u64)> = list
        .iter()
        .filter_map(|(p, t)| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .ok()
                .map(|d| (p.to_string_lossy().to_string(), d.as_secs()))
        })
        .collect();
    if let Ok(json) = serde_json::to_string(&data) {
        let _ = std::fs::write(path, json);
    }
}

// ---------- 规则管理 ----------

#[tauri::command]
fn get_rules(state: State<'_, AppState>) -> Vec<Rule> {
    state.store.lock().unwrap().all()
}

#[tauri::command]
fn save_rule(state: State<'_, AppState>, app: AppHandle, rule: Rule) -> Rule {
    let saved = state.store.lock().unwrap().upsert(rule);
    refresh_monitoring(state.inner(), &app);
    saved
}

#[tauri::command]
fn delete_rule(state: State<'_, AppState>, app: AppHandle, id: String) -> bool {
    let changed = state.store.lock().unwrap().delete(&id);
    if changed {
        refresh_monitoring(state.inner(), &app);
    }
    changed
}

#[tauri::command]
fn set_rule_enabled(state: State<'_, AppState>, app: AppHandle, id: String, enabled: bool) -> bool {
    let changed = state.store.lock().unwrap().set_enabled(&id, enabled);
    if changed {
        refresh_monitoring(state.inner(), &app);
    }
    changed
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

fn apply_rule_inner(
    state: &AppState,
    app: &AppHandle,
    id: &str,
) -> Result<Vec<OpRecord>, String> {
    let rule = state
        .store
        .lock()
        .unwrap()
        .get(id)
        .ok_or_else(|| "规则不存在".to_string())?;

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
fn apply_rule_now(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<Vec<OpRecord>, String> {
    apply_rule_inner(state.inner(), &app, &id)
}

#[tauri::command]
fn get_oplog(state: State<'_, AppState>, limit: usize) -> Vec<OpRecord> {
    oplog::read_recent(&state.oplog_path, limit.min(500))
}

fn undo_last_inner(state: &AppState, app: &AppHandle) -> Result<usize, String> {
    let undone = oplog::undo_last_batch(&state.oplog_path).map_err(|e| e.to_string())?;
    if !undone.is_empty() {
        // 记入撤销保护名单：监控不再自动处理这些文件（除非它们之后被修改/重新下载）
        let now = SystemTime::now();
        let mut skip = state.recently_undone.lock().unwrap();
        for r in &undone {
            let p = PathBuf::from(&r.src);
            skip.retain(|(ep, _)| ep != &p);
            skip.push((p, now));
        }
        save_skip_list(&state.skip_list_path, &skip);
        drop(skip);
        let _ = app.emit("fp://oplog-updated", ());
    }
    Ok(undone.len())
}

#[tauri::command]
fn undo_last(state: State<'_, AppState>, app: AppHandle) -> Result<usize, String> {
    undo_last_inner(state.inner(), &app)
}

// ---------- 监控 ----------

/// 仅在全局监控运行时重新注册来源文件夹；暂停期间保留单规则配置。
/// 调用方必须先释放 store 锁，避免等待监控线程退出时阻塞其回调。
fn refresh_monitoring(state: &AppState, app: &AppHandle) {
    let running = *state.monitoring.lock().unwrap();
    if running {
        let running = start_monitoring_inner(state, app);
        let _ = app.emit("fp://monitoring-changed", running);
    }
}

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

fn start_monitoring_inner(state: &AppState, app: &AppHandle) -> bool {
    stop_monitoring_inner(state);

    let folders = watched_folders(state);
    if folders.is_empty() {
        return false;
    }

    let app2 = app.clone();
    let oplog_path = state.oplog_path.clone();
    let callback: watcher::ApplyCallback = Box::new(move |folder| {
        let app_state: State<'_, AppState> = app2.state();
        // 快照撤销保护名单，并清理已失效条目（文件已删除或撤销后又被修改过）
        let skip_list: Vec<(PathBuf, SystemTime)> = {
            let mut guard = app_state.recently_undone.lock().unwrap();
            guard.retain(|(p, t)| {
                std::fs::metadata(p.as_path())
                    .and_then(|m| m.modified())
                    .map(|mt| mt <= *t)
                    .unwrap_or(false)
            });
            guard.clone()
        };
        let should_skip = |path: &Path| -> bool {
            match std::fs::metadata(path).and_then(|m| m.modified()) {
                Ok(mt) => skip_list
                    .iter()
                    .any(|(p, t)| oplog::paths_equal(p, path) && mt <= *t),
                Err(_) => false,
            }
        };
        let rules = app_state.store.lock().unwrap().all();
        let results = watcher::apply_rules_for_folder(&rules, folder, &should_skip);
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
    *state.monitoring.lock().unwrap() = true;
    true
}

fn stop_monitoring_inner(state: &AppState) {
    if let Some(handle) = state.watcher.lock().unwrap().take() {
        handle.shutdown();
    }
    *state.monitoring.lock().unwrap() = false;
}

#[tauri::command]
fn start_monitoring(state: State<'_, AppState>, app: AppHandle) -> bool {
    let ok = start_monitoring_inner(state.inner(), &app);
    let _ = app.emit("fp://monitoring-changed", ok);
    ok
}

#[tauri::command]
fn stop_monitoring(state: State<'_, AppState>, app: AppHandle) {
    stop_monitoring_inner(state.inner());
    let _ = app.emit("fp://monitoring-changed", false);
}

#[tauri::command]
fn is_monitoring(state: State<'_, AppState>) -> bool {
    *state.monitoring.lock().unwrap()
}

// ---------- 开机自启 ----------

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    if enabled {
        al.enable().map_err(|e| e.to_string())?;
    } else {
        al.disable().map_err(|e| e.to_string())?;
    }
    Ok(enabled)
}

#[tauri::command]
fn is_autostart_enabled(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
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

// ---------- 托盘 ----------

fn build_tray(app: &App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle_watch", "暂停/恢复监控", true, None::<&str>)?;
    let undo = MenuItem::with_id(app, "undo", "撤销上次整理", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &undo, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("FilePilot — 先预览，再动手，可撤销")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "toggle_watch" => {
                let state: State<'_, AppState> = app.state();
                let monitoring = *state.monitoring.lock().unwrap();
                if monitoring {
                    stop_monitoring_inner(state.inner());
                } else {
                    start_monitoring_inner(state.inner(), app);
                }
                let _ = app.emit(
                    "fp://monitoring-changed",
                    *state.monitoring.lock().unwrap(),
                );
            }
            "undo" => {
                let state: State<'_, AppState> = app.state();
                let _ = undo_last_inner(state.inner(), app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

// ---------- 入口 ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = app
                .path()
                .app_local_data_dir()
                .expect("无法获取应用数据目录");
            let _ = std::fs::create_dir_all(&data_dir);
            let state = AppState::new(
                data_dir.join("rules.json"),
                data_dir.join("oplog.jsonl"),
                data_dir.join("skip-list.json"),
            );
            app.manage(state);
            build_tray(app)?;

            // 启动时自动开始监控
            let handle = app.handle().clone();
            let state_ref: State<'_, AppState> = handle.state();
            start_monitoring_inner(state_ref.inner(), &handle);

            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭窗口时最小化到托盘，而不是退出
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
            is_monitoring,
            set_autostart,
            is_autostart_enabled,
            pick_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
