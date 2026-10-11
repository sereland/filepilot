//! FilePilot 主入口：应用状态 + Tauri commands。
//!
//! 核心逻辑在 filepilot-core crate（可独立测试），这里只做胶水层。

use filepilot_core::protection::{protect_restored, protection, unchanged, ProtectedFile};
use filepilot_core::{engine, oplog, rules, store, watcher};
use oplog::OpRecord;
use rules::Rule;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use store::RuleStore;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    store: Mutex<RuleStore>,
    oplog_path: PathBuf,
    watcher: Mutex<Option<watcher::WatchHandle>>,
    monitoring: Mutex<bool>,
    /// 撤销/整理保护：文件状态及目录内部条目状态，持久化到磁盘。
    /// 跳过未变化的恢复项，以及复制来源和重命名目标，避免事件反馈循环。
    recently_undone: Mutex<Vec<ProtectedFile>>,
    skip_list_path: PathBuf,
    operations: Mutex<()>,
    preview_times: Mutex<std::collections::HashMap<String, std::time::SystemTime>>,
}

fn protect_failed_archives(state: &AppState, records: &[OpRecord]) {
    let mut skip = state.recently_undone.lock().unwrap();
    for record in records
        .iter()
        .filter(|r| r.kind == "move_extract" && r.error.is_some())
    {
        for path in &record.restored_sources {
            protect_restored(Path::new(path), &mut skip);
        }
    }
    save_skip_list(&state.skip_list_path, &skip);
}

fn protect_organized_objects(state: &AppState, records: &[OpRecord]) {
    let mut skip = state.recently_undone.lock().unwrap();
    filepilot_core::protection::protect_organized(records, &mut skip);
    save_skip_list(&state.skip_list_path, &skip);
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
            operations: Mutex::new(()),
            preview_times: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

/// 加载文件状态保护名单，兼容此前按撤销时间保存的名单。
fn load_skip_list(path: &Path) -> Vec<ProtectedFile> {
    let data = std::fs::read_to_string(path).unwrap_or_default();
    if data.trim().is_empty() {
        return Vec::new();
    }
    if let Ok(files) = serde_json::from_str::<Vec<ProtectedFile>>(&data) {
        return files;
    }
    serde_json::from_str::<Vec<(String, u64)>>(data.as_str())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(p, _)| protection(Path::new(&p)))
        .collect()
}

/// 持久化撤销保护名单，失败时静默忽略（下次撤销会重试）
fn save_skip_list(path: &Path, list: &[ProtectedFile]) {
    if let Ok(json) = serde_json::to_string(list) {
        let _ = std::fs::write(path, json);
    }
}

// ---------- 规则管理 ----------

#[tauri::command]
fn get_rules(state: State<'_, AppState>) -> Vec<Rule> {
    state.store.lock().unwrap().all()
}

#[tauri::command]
fn save_rule(state: State<'_, AppState>, app: AppHandle, rule: Rule) -> Result<Rule, String> {
    rule.validate()?;
    let saved = state.store.lock().unwrap().upsert(rule);
    state.preview_times.lock().unwrap().remove(&saved.id);
    refresh_monitoring(state.inner(), &app);
    Ok(saved)
}

#[tauri::command]
fn delete_rule(state: State<'_, AppState>, app: AppHandle, id: String) -> bool {
    let changed = state.store.lock().unwrap().delete(&id);
    if changed {
        state.preview_times.lock().unwrap().remove(&id);
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
fn open_rule_folder(
    state: State<'_, AppState>,
    app: AppHandle,
    rule_id: String,
    folder: String,
) -> Result<(), String> {
    let rule = state
        .store
        .lock()
        .unwrap()
        .get(&rule_id)
        .ok_or_else(|| "规则不存在，请刷新后重试。".to_string())?;
    let path = Path::new(&folder);
    let allowed = rule
        .watch_folders
        .iter()
        .any(|source| oplog::paths_equal(Path::new(source), path))
        || rule.actions.iter().any(|action| match action {
            rules::Action::Move { dest }
            | rules::Action::Copy { dest }
            | rules::Action::MoveAndExtract { dest, .. } => {
                oplog::paths_equal(Path::new(dest), path)
            }
            rules::Action::Organize { dest, .. } => oplog::paths_equal(Path::new(dest), path),
            _ => false,
        });
    if !allowed {
        return Err("该文件夹不属于此规则，请刷新后重试。".to_string());
    }
    if !path.is_dir() {
        return Err("文件夹不存在或无法访问，请检查路径。".to_string());
    }
    app.opener()
        .open_path(folder, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn preview_rule(app: AppHandle, id: String) -> Result<Vec<engine::PlanItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state: State<'_, AppState> = app.state();
        let _operation = state.operations.lock().unwrap();
        let rule = state
            .store
            .lock()
            .unwrap()
            .get(&id)
            .ok_or_else(|| "规则不存在".to_string())?;
        let folders: Vec<PathBuf> = rule.watch_folders.iter().map(PathBuf::from).collect();
        let at = std::time::SystemTime::now();
        let plan = engine::dry_run_at(&rule, &folders, at);
        state
            .preview_times
            .lock()
            .unwrap()
            .insert(rule.id.clone(), at);
        Ok(plan)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn apply_rule_inner(
    state: &AppState,
    app: &AppHandle,
    id: &str,
    selected_sources: &[String],
    preview_items: Option<&[engine::PlanItem]>,
) -> Result<Vec<OpRecord>, String> {
    let _operation = state.operations.lock().unwrap();
    if selected_sources.is_empty() {
        return Ok(Vec::new());
    }
    let rule = state
        .store
        .lock()
        .unwrap()
        .get(id)
        .ok_or_else(|| "规则不存在".to_string())?;

    let folders: Vec<PathBuf> = rule.watch_folders.iter().map(PathBuf::from).collect();
    let at = preview_items
        .and_then(|_| state.preview_times.lock().unwrap().get(id).copied())
        .unwrap_or_else(std::time::SystemTime::now);
    let plan = engine::confirmed_dry_run(&rule, &folders, selected_sources, preview_items, at)?;
    if plan.is_empty() {
        return Ok(Vec::new());
    }
    let batch_id = uuid::Uuid::new_v4().to_string();
    let records = engine::apply_plan(&plan, &batch_id, &rule.id);
    protect_failed_archives(state, &records);
    protect_organized_objects(state, &records);
    if !records.is_empty() {
        oplog::append_records(&state.oplog_path, &records)
            .map_err(|e| format!("写入日志失败: {}", e))?;
        let _ = app.emit("fp://oplog-updated", ());
    }
    Ok(records)
}

#[tauri::command]
async fn apply_rule_now(
    app: AppHandle,
    id: String,
    selected_sources: Vec<String>,
    preview_items: Option<Vec<engine::PlanItem>>,
) -> Result<Vec<OpRecord>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state: State<'_, AppState> = app.state();
        apply_rule_inner(
            state.inner(),
            &app,
            &id,
            &selected_sources,
            preview_items.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_rule_last_runs(
    state: State<'_, AppState>,
) -> std::collections::HashMap<String, oplog::RuleLastRun> {
    oplog::rule_last_runs(&state.oplog_path)
}

#[tauri::command]
fn get_oplog(state: State<'_, AppState>, limit: usize) -> Vec<OpRecord> {
    oplog::read_recent(&state.oplog_path, limit.min(500))
}

#[tauri::command]
fn get_oplog_batch(state: State<'_, AppState>, batch_id: String) -> Vec<OpRecord> {
    oplog::read_batch(&state.oplog_path, &batch_id)
}

fn undo_last_inner(state: &AppState, app: &AppHandle) -> Result<usize, String> {
    let _operation = state.operations.lock().unwrap();
    let undone = oplog::undo_last_batch(&state.oplog_path).map_err(|e| e.to_string())?;
    if !undone.is_empty() {
        // 记入撤销保护名单：监控不再自动处理这些文件（除非它们之后被修改/重新下载）
        let mut skip = state.recently_undone.lock().unwrap();
        for r in &undone {
            if !r.restored_state.is_empty() {
                for (path, size, modified) in &r.restored_state {
                    let file = ProtectedFile {
                        path: PathBuf::from(path),
                        size: *size,
                        modified: *modified,
                        directory_state: None,
                    };
                    if unchanged(&file) {
                        skip.retain(|old| !oplog::paths_equal(&old.path, &file.path));
                        skip.push(file);
                    }
                }
                for path in &r.restored_sources {
                    if Path::new(path).is_dir() {
                        protect_restored(Path::new(path), &mut skip);
                    }
                }
            } else if r.restored_sources.is_empty() {
                protect_restored(Path::new(&r.src), &mut skip);
            } else {
                for path in &r.restored_sources {
                    protect_restored(Path::new(path), &mut skip);
                }
            }
        }
        save_skip_list(&state.skip_list_path, &skip);
        drop(skip);
        let _ = app.emit("fp://oplog-updated", ());
    }
    Ok(undone.len())
}

#[tauri::command]
async fn undo_last(app: AppHandle) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state: State<'_, AppState> = app.state();
        undo_last_inner(state.inner(), &app)
    })
    .await
    .map_err(|e| e.to_string())?
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
        let _operation = app_state.operations.lock().unwrap();
        // 快照撤销保护名单，并清理已失效条目（文件已删除或撤销后又被修改过）
        let skip_list: Vec<ProtectedFile> = {
            let mut guard = app_state.recently_undone.lock().unwrap();
            guard.retain(unchanged);
            guard.clone()
        };
        let should_skip = |path: &Path| -> bool {
            skip_list
                .iter()
                .any(|file| oplog::paths_equal(&file.path, path) && unchanged(file))
        };
        let rules = app_state.store.lock().unwrap().all();
        let results = watcher::apply_rules_for_folder(&rules, folder, &should_skip);
        let mut total = 0;
        for (_rule_id, records) in &results {
            total += records.len();
            protect_failed_archives(app_state.inner(), records);
            protect_organized_objects(app_state.inner(), records);
            let _ = oplog::append_records(&oplog_path, records);
            if let Some(message) = records
                .iter()
                .find_map(|r| r.error.as_ref().or(r.warning.as_ref()))
            {
                let _ = app2.emit("fp://operation-error", message.clone());
            }
        }
        if total > 0 {
            let _ = app2.emit("fp://oplog-updated", total);
        }
        total
    });

    let rules = state.store.lock().unwrap().all();
    let modes = folders
        .into_iter()
        .map(|folder| {
            let recursive = rules.iter().any(|rule| {
                rule.enabled
                    && rule.target_kind == rules::TargetKind::Directory
                    && rule
                        .watch_folders
                        .iter()
                        .any(|source| oplog::paths_equal(Path::new(source), &folder))
            });
            (folder, recursive)
        })
        .collect();
    let handle = watcher::start_watching_with_modes(modes, callback);
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
                let _ = app.emit("fp://monitoring-changed", *state.monitoring.lock().unwrap());
            }
            "undo" => {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state: State<'_, AppState> = app.state();
                    if let Err(e) = undo_last_inner(state.inner(), &app) {
                        let _ = app.emit("fp://operation-error", e);
                    }
                });
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
            // Recover journaled interrupted package operations before monitoring starts.
            let mut archive_roots = HashSet::new();
            for rule in state_ref.store.lock().unwrap().all() {
                for action in rule.actions {
                    if let rules::Action::MoveAndExtract { dest, .. } = action {
                        archive_roots.insert(dest);
                    }
                }
            }
            for root in archive_roots {
                match filepilot_core::archive::recover(Path::new(&root)) {
                    Ok(records) => {
                        let _ = oplog::reconcile_archive_records(&state_ref.oplog_path, &records);
                        let mut skip = state_ref.recently_undone.lock().unwrap();
                        for record in records.iter().filter(|r| r.undone) {
                            for (path, size, modified) in &record.restored_state {
                                let file = ProtectedFile {
                                    path: PathBuf::from(path),
                                    size: *size,
                                    modified: *modified,
                                    directory_state: None,
                                };
                                if unchanged(&file) {
                                    skip.retain(|old| !oplog::paths_equal(&old.path, &file.path));
                                    skip.push(file);
                                }
                            }
                        }
                        save_skip_list(&state_ref.skip_list_path, &skip);
                    }
                    Err(e) => eprintln!("包更新恢复未完成：{e}"),
                }
            }
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
            open_rule_folder,
            preview_rule,
            apply_rule_now,
            get_oplog,
            get_rule_last_runs,
            get_oplog_batch,
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
