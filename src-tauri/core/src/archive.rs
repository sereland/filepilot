//! ZIP packages are one reversible operation: prepare, journal moves, publish, retire.
use crate::rules::{ArchiveRetention, FileMeta};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const MANAGEMENT: &str = ".filepilot-backups";
const MAX_ENTRIES: usize = 100_000;
const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;

struct PreparedDirectory(PathBuf);
impl Drop for PreparedDirectory {
    fn drop(&mut self) {
        // No user content is moved before the durable manifest exists.
        if !self.0.join("manifest.json").exists() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchivePlan {
    pub root: String,
    pub retention: ArchiveRetention,
    pub archive_dest: String,
    pub folder_dest: String,
    pub obsolete_sources: Vec<String>,
    pub old_targets: Vec<String>,
    pub cleanup_backup: Option<String>,
    pub error: Option<String>,
    pub expected_state: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MoveStep {
    from: PathBuf,
    to: PathBuf,
    digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    id: String,
    phase: String,
    latest: bool,
    head: PathBuf,
    previous: Option<PathBuf>,
    completed: usize,
    steps: Vec<MoveStep>,
    archive_dest: PathBuf,
    folder_dest: PathBuf,
    archive_digest: String,
    folder_digest: String,
    #[serde(default)]
    record: Option<crate::oplog::OpRecord>,
    #[serde(default)]
    restored_state: Vec<(String, u64, std::time::SystemTime)>,
}

fn fail(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

/// Browser download suffixes only. Other package names remain separate groups.
pub fn package_name(stem: &str) -> String {
    let stem = stem.trim();
    if let Some(prefix) = stem.strip_suffix(')') {
        if let Some((base, number)) = prefix.rsplit_once('(') {
            if !number.is_empty()
                && number.bytes().all(|c| c.is_ascii_digit())
                && !base.trim_end().is_empty()
            {
                return base.trim_end().to_string();
            }
        }
    }
    stem.to_string()
}

fn key(name: &str) -> String {
    name.to_lowercase()
}
pub(crate) fn valid_component(name: &str) -> bool {
    let reserved = name.split('.').next().unwrap_or("").to_uppercase();
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        && ![
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(&reserved.as_str())
        && name.chars().count() <= 180
}

fn package_root(root: &Path, name: &str) -> PathBuf {
    let hash = format!("{:x}", Sha256::digest(key(name).as_bytes()));
    root.join(MANAGEMENT).join(hash)
}

fn old_targets(root: &Path, name: &str) -> io::Result<Vec<String>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let stem = if path.is_dir() {
            Some(file_name.clone())
        } else if path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("zip"))
            .unwrap_or(false)
        {
            path.file_stem().map(|s| s.to_string_lossy().to_string())
        } else {
            None
        };
        if stem
            .map(|s| key(&package_name(&s)) == key(name))
            .unwrap_or(false)
        {
            paths.push(path.to_string_lossy().to_string());
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn plan(
    meta: &FileMeta,
    root: &str,
    retention: &ArchiveRetention,
    obsolete_sources: Vec<String>,
    reserved: &mut HashSet<String>,
) -> ArchivePlan {
    let original = Path::new(&meta.name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let latest = *retention == ArchiveRetention::Latest;
    let base = if latest {
        package_name(&original)
    } else {
        original
    };
    let mut name = base.clone();
    let root_path = Path::new(root);
    let mut number = 1;
    if !latest {
        while root_path.join(format!("{name}.zip")).exists()
            || root_path.join(&name).exists()
            || reserved.contains(&key(&name))
        {
            name = format!("{base} ({number})");
            number += 1;
        }
    }
    reserved.insert(key(&name));
    let old = if latest {
        old_targets(root_path, &base)
    } else {
        Ok(Vec::new())
    };
    let head = package_root(root_path, &base).join("current.json");
    let previous = read_head(&head).ok().flatten();
    let mut error = old.as_ref().err().map(|e| e.to_string());
    if meta.ext != "zip" {
        error = Some("移动并解压仅支持 ZIP 文件。".into());
    }
    if !valid_component(&name)
        || !root_path.is_absolute()
        || root_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        error = Some("请选择不含 .. 的绝对目标路径，包名不能包含非法字符。".into());
    }
    if meta
        .path
        .parent()
        .map(|p| crate::oplog::paths_equal(p, root_path))
        .unwrap_or(false)
    {
        error = Some("目标文件夹不能与来源文件夹相同。".into());
    }
    let mut expected_state = BTreeMap::new();
    for path in std::iter::once(meta.path.to_string_lossy().to_string())
        .chain(obsolete_sources.iter().cloned())
        .chain(old.as_ref().ok().into_iter().flatten().cloned())
    {
        match fingerprint(Path::new(&path)) {
            Ok(digest) => {
                expected_state.insert(path, digest);
            }
            Err(e) => {
                error = Some(format!("无法读取文件状态：{e}"));
            }
        }
    }
    ArchivePlan {
        root: root.to_string(),
        retention: retention.clone(),
        archive_dest: root_path
            .join(format!("{name}.zip"))
            .to_string_lossy()
            .to_string(),
        folder_dest: root_path.join(&name).to_string_lossy().to_string(),
        obsolete_sources,
        old_targets: old.unwrap_or_default(),
        cleanup_backup: previous.map(|p| p.parent().unwrap_or(&p).to_string_lossy().to_string()),
        error,
        expected_state,
    }
}

fn read_head(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let id: String = serde_json::from_slice(&fs::read(path)?).map_err(|e| fail(e.to_string()))?;
    uuid::Uuid::parse_str(&id).map_err(|_| fail("更新状态损坏，请检查备份目录。"))?;
    Ok(Some(path.parent().unwrap().join(id).join("manifest.json")))
}

fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| fail(e.to_string()))?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("写入更新状态失败：{}（{e}）", path.display()),
        )
    })
}

fn read_manifest(path: &Path) -> io::Result<Manifest> {
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(path)?).map_err(|e| fail(format!("更新记录损坏：{e}")))?;
    let tx = path.parent().ok_or_else(|| fail("更新记录路径无效"))?;
    let package = tx.parent().ok_or_else(|| fail("更新记录路径无效"))?;
    let managed = package.parent().ok_or_else(|| fail("更新记录路径无效"))?;
    let root = managed.parent().ok_or_else(|| fail("更新记录路径无效"))?;
    let steps: Vec<MoveStep> = if path.file_name().and_then(|n| n.to_str()) == Some("undo.json") {
        manifest
            .steps
            .iter()
            .rev()
            .map(|s| MoveStep {
                from: s.to.clone(),
                to: s.from.clone(),
                digest: s.digest.clone(),
            })
            .collect()
    } else {
        manifest.steps.clone()
    };
    if steps.len() < 2
        || manifest.completed > steps.len()
        || uuid::Uuid::parse_str(&manifest.id).is_err()
        || tx.file_name().and_then(|n| n.to_str()) != Some(manifest.id.as_str())
        || managed.file_name().and_then(|n| n.to_str()) != Some(MANAGEMENT)
        || manifest.head != package.join("current.json")
        || manifest.archive_dest.parent() != Some(root)
        || manifest.folder_dest.parent() != Some(root)
        || steps.last().unwrap().from != tx.join("extracted")
        || steps.last().unwrap().to != manifest.folder_dest
        || steps[steps.len() - 2].to != manifest.archive_dest
        || steps.iter().any(|s| {
            !s.from.is_absolute()
                || !s.to.is_absolute()
                || s.from
                    .components()
                    .chain(s.to.components())
                    .any(|c| matches!(c, std::path::Component::ParentDir))
        })
        || steps[..steps.len() - 2]
            .iter()
            .any(|s| !s.to.starts_with(tx.join("previous")))
        || manifest
            .previous
            .as_ref()
            .map(|p| {
                p.parent().and_then(Path::parent) != Some(package)
                    || p.file_name().and_then(|n| n.to_str()) != Some("manifest.json")
                    || p.parent()
                        .and_then(Path::file_name)
                        .and_then(|n| n.to_str())
                        .and_then(|id| uuid::Uuid::parse_str(id).ok())
                        .is_none()
            })
            .unwrap_or(false)
    {
        return Err(fail("更新记录路径校验失败，请保留备份并手动检查。"));
    }
    Ok(manifest)
}

/// Never follow a symlink/junction, including ancestor directories.
pub(crate) fn no_links(path: &Path) -> io::Result<()> {
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(md) => {
                #[cfg(windows)]
                let reparse = {
                    use std::os::windows::fs::MetadataExt;
                    md.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let reparse = false;
                if md.file_type().is_symlink() || reparse {
                    return Err(fail("路径包含符号链接或目录联接，请选择普通文件夹。"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub fn fingerprint(path: &Path) -> io::Result<String> {
    no_links(path)?;
    let md = fs::metadata(path)?;
    let mut hash = Sha256::new();
    if md.is_file() {
        hash.update(b"file\0");
        let mut file = File::open(path)?;
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
    } else if md.is_dir() {
        hash.update(b"dir\0");
        let mut entries: Vec<_> = fs::read_dir(path)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            hash.update(entry.file_name().to_string_lossy().as_bytes());
            hash.update([0]);
            hash.update(fingerprint(&entry.path())?.as_bytes());
            hash.update([0]);
        }
    } else {
        return Err(fail("不支持特殊文件。"));
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn extract_zip(src: &Path, dest: &Path) -> io::Result<()> {
    let mut zip =
        zip::ZipArchive::new(File::open(src)?).map_err(|e| fail(format!("无法读取 ZIP：{e}")))?;
    if zip.is_empty() {
        return Err(fail("ZIP 为空，未替换旧版。"));
    }
    if zip.len() > MAX_ENTRIES {
        return Err(fail("ZIP 条目超过 100000 项。"));
    }
    let mut total = 0u64;
    let mut seen = HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| fail(e.to_string()))?;
        if entry.is_symlink() || entry.encrypted() {
            return Err(fail("暂不支持含符号链接或加密的 ZIP。"));
        }
        let name = entry
            .name()
            .map_err(|e| fail(e.to_string()))?
            .replace('\\', "/");
        let trimmed = name.trim_end_matches('/');
        if entry.enclosed_name().is_none()
            || trimmed.split('/').any(|p| !valid_component(p))
            || !seen.insert(key(trimmed))
        {
            return Err(fail(format!("ZIP 包含不安全或重复的路径：{name}")));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| fail("ZIP 解压体积过大。"))?;
        if total > MAX_BYTES {
            return Err(fail("ZIP 解压体积超过 8 GB。"));
        }
        let out = dest.join(trimmed);
        if entry.is_dir() {
            fs::create_dir_all(out)?;
        } else {
            fs::create_dir_all(out.parent().unwrap())?;
            let mut file = OpenOptions::new().create_new(true).write(true).open(out)?;
            let limit = entry.size();
            let mut bounded = (&mut entry).take(limit.saturating_add(1));
            let written = io::copy(&mut bounded, &mut file)?;
            if written != limit {
                return Err(fail("ZIP 内容大小或校验不一致。"));
            }
            file.sync_all()?;
        }
    }
    Ok(())
}

pub(crate) fn relocate(from: &Path, to: &Path, expected: &str) -> io::Result<()> {
    no_links(from)?;
    no_links(to)?;
    #[cfg(windows)]
    ensure_not_in_use(from)?;
    if to.exists() {
        return Err(fail(format!("目标已被占用：{}", to.display())));
    }
    if fingerprint(from)? != expected {
        return Err(fail(format!("文件已变化：{}", from.display())));
    }
    let parent = to.parent().unwrap();
    fs::create_dir_all(parent).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("创建目标目录失败：{}（{e}）", parent.display()),
        )
    })?;
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(original) if from.is_file() => {
            // Cross-volume copy never replaces an occupied destination.
            let mut source = File::open(from)?;
            let mut target = OpenOptions::new().create_new(true).write(true).open(to)?;
            io::copy(&mut source, &mut target)?;
            target.sync_all()?;
            drop(target);
            drop(source);
            if fingerprint(to)? != expected || fingerprint(from)? != expected {
                return Err(fail("移动时文件发生变化，保留两份文件，请检查路径。"));
            }
            fs::remove_file(from)
                .map_err(|e| fail(format!("移动源文件失败：{e}；原错误：{original}")))
        }
        Err(e) => Err(io::Error::new(e.kind(), format!("移动文件夹失败：{} → {}（{e}）；请关闭该文件夹及子文件夹的窗口、预览或解压工具后重新预览并执行。", from.display(), to.display()))),
    }
}

#[cfg(windows)]
fn ensure_not_in_use(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    no_links(path)?;
    if path.is_dir() {
        // Windows refuses to rename a parent tree with an open descendant
        // directory, even when that handle allows DELETE sharing. Checking only
        // files misses Explorer/picker/working-directory handles.
        File::options().access_mode(1).share_mode(0)
            .custom_flags(0x02000000).open(path)
            .map_err(|e| fail(format!("文件夹被占用或无法访问：{}（{e}）；请关闭该文件夹的窗口、预览或解压工具后重新预览并执行。", path.display())))?;
        for entry in fs::read_dir(path)? {
            ensure_not_in_use(&entry?.path())?;
        }
    } else {
        File::options()
            .read(true)
            .share_mode(0)
            .open(path)
            .map_err(|e| fail(format!("文件被占用或无法访问：{}（{e}）", path.display())))?;
    }
    Ok(())
}

fn rollback(manifest: &Manifest) -> io::Result<()> {
    // Include the in-flight move: process termination may precede journal flush.
    for (index, step) in manifest
        .steps
        .iter()
        .enumerate()
        .take((manifest.completed + 1).min(manifest.steps.len()))
        .rev()
    {
        if !step.to.exists() {
            continue;
        }
        if index == manifest.completed
            && step.from.exists()
            && fingerprint(&step.to)? != step.digest
        {
            continue;
        }
        if fingerprint(&step.to)? != step.digest {
            return Err(fail(format!(
                "恢复位置的内容已变化，备份保留在：{}",
                step.to.display()
            )));
        }
        if step.from.exists() {
            if step.from.is_file() && step.to.is_file() && fingerprint(&step.from)? == step.digest {
                fs::remove_file(&step.to)?;
            } else {
                return Err(fail(format!("恢复位置已被占用：{}", step.from.display())));
            }
        } else {
            relocate(&step.to, &step.from, &step.digest)?;
        }
    }
    Ok(())
}

/// Recover only journaled, interrupted operations; preview never calls this.
pub fn recover(root: &Path) -> io::Result<Vec<crate::oplog::OpRecord>> {
    recover_with_recycler(root, &|path| {
        trash::delete(path).map_err(|e| fail(e.to_string()))
    })
}

fn recover_with_recycler(
    root: &Path,
    recycle: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<Vec<crate::oplog::OpRecord>> {
    let managed = root.join(MANAGEMENT);
    if !managed.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    let mut pending_cleanup = Vec::new();
    no_links(&managed)?;
    for package in fs::read_dir(&managed)? {
        let package = package?.path();
        if !package.is_dir() {
            continue;
        }
        no_links(&package)?;
        for tx in fs::read_dir(package)? {
            let path = tx?.path().join("manifest.json");
            if !path.is_file() {
                continue;
            }
            no_links(&path)?;
            let mut manifest = read_manifest(&path)?;
            let undo_path = path.with_file_name("undo.json");
            if undo_path.exists() {
                let undo = read_manifest(&undo_path)?;
                if manifest.phase != "undone" {
                    rollback(&undo)?;
                    if manifest.latest {
                        write_json(&manifest.head, &manifest.id)?;
                    }
                }
                fs::remove_file(&undo_path)?;
            }
            if manifest.phase == "applying" {
                rollback(&manifest)?;
                if manifest.latest && read_head(&manifest.head)?.as_deref() == Some(path.as_path())
                {
                    restore_head(&manifest)?;
                }
                manifest.phase = "aborted".into();
                write_json(&path, &manifest)?;
            } else if manifest.phase == "undoing" {
                return Err(fail(format!(
                    "撤销被中断，请保留备份并检查：{}",
                    path.display()
                )));
            }
            if manifest.phase == "committed" || manifest.phase == "undone" {
                if manifest.latest
                    && manifest.phase == "committed"
                    && read_head(&manifest.head)?.as_deref() == Some(path.as_path())
                {
                    pending_cleanup.push((path.clone(), manifest.clone()));
                }
                if let Some(mut record) = manifest.record.clone() {
                    record.undone = manifest.phase == "undone";
                    if record.undone {
                        record.restored_sources = restored_paths(&manifest)
                            .into_iter()
                            .map(|p| p.to_string_lossy().to_string())
                            .collect();
                        record.restored_state = manifest.restored_state.clone();
                    }
                    records.push(record);
                }
            }
        }
    }
    // A crash can occur after publication but before the older snapshot is recycled.
    // Complete recovery before retiring anything, so an interrupted newer update can
    // still roll back to this operation's head.
    for (path, mut manifest) in pending_cleanup {
        let warning = retire_previous(&manifest, recycle)
            .err()
            .map(|e| format!("更新成功；旧备份清理失败，已保留：{e}"));
        if let Some(record) = &mut manifest.record {
            if record.warning != warning {
                record.warning = warning.clone();
                if let Some(recovered) = records.iter_mut().find(|r| r.id == record.id) {
                    recovered.warning = warning;
                }
                write_json(&path, &manifest)?;
            }
        }
    }
    Ok(records)
}

pub fn execute(src: &str, plan: &ArchivePlan) -> io::Result<(String, Option<String>)> {
    execute_with_recycler(src, plan, &|path| {
        trash::delete(path).map_err(|e| fail(e.to_string()))
    })
}

pub fn execute_record(
    src: &str,
    plan: &ArchivePlan,
    record: &crate::oplog::OpRecord,
) -> io::Result<(String, Option<String>)> {
    execute_with_context(
        src,
        plan,
        &|path| trash::delete(path).map_err(|e| fail(e.to_string())),
        Some(record),
    )
}

fn execute_with_recycler(
    src: &str,
    plan: &ArchivePlan,
    recycle: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<(String, Option<String>)> {
    execute_with_context(src, plan, recycle, None)
}

fn execute_with_context(
    src: &str,
    plan: &ArchivePlan,
    recycle: &dyn Fn(&Path) -> io::Result<()>,
    record: Option<&crate::oplog::OpRecord>,
) -> io::Result<(String, Option<String>)> {
    if let Some(error) = &plan.error {
        return Err(fail(error));
    }
    let root = Path::new(&plan.root);
    no_links(root)?;
    recover_with_recycler(root, recycle)?;
    for (path, expected) in &plan.expected_state {
        if fingerprint(Path::new(path))? != *expected {
            return Err(fail("文件或旧目录已变化，请重新预览。"));
        }
    }
    let latest = plan.retention == ArchiveRetention::Latest;
    let dest = Path::new(&plan.archive_dest);
    let folder = Path::new(&plan.folder_dest);
    let name = folder.file_name().unwrap().to_string_lossy();
    if latest && old_targets(root, &name)? != plan.old_targets {
        return Err(fail("目标内容已变化，请重新预览。"));
    }
    if !latest && (dest.exists() || folder.exists()) {
        return Err(fail("预览目标已被占用，请重新预览。"));
    }
    let package = package_root(root, &name);
    no_links(&package)?;
    fs::create_dir_all(&package)?;
    let head = package.join("current.json");
    let previous = if latest { read_head(&head)? } else { None };
    let id = uuid::Uuid::new_v4().to_string();
    let tx = package.join(&id);
    fs::create_dir(&tx)?;
    let _prepared = PreparedDirectory(tx.clone());
    let new_folder = tx.join("extracted");
    fs::create_dir(&new_folder)?;
    // Extract from a verified private copy, before any existing data is moved.
    let staged_zip = tx.join("incoming.zip");
    let prepared = (|| -> io::Result<(String, String)> {
        let source_digest = fingerprint(Path::new(src))?;
        if plan.expected_state.get(src) != Some(&source_digest) {
            return Err(fail("下载包已变化，请重新预览。"));
        }
        fs::copy(src, &staged_zip)?;
        if fingerprint(&staged_zip)? != source_digest {
            return Err(fail("下载包仍在变化，请稍后重试。"));
        }
        extract_zip(&staged_zip, &new_folder)?;
        Ok((source_digest, fingerprint(&new_folder)?))
    })();
    let (archive_digest, folder_digest) = match prepared {
        Ok(value) => value,
        Err(e) => {
            let _ = fs::remove_dir_all(&tx);
            return Err(e);
        }
    };
    let mut steps = Vec::new();
    for (index, path) in plan
        .old_targets
        .iter()
        .chain(&plan.obsolete_sources)
        .enumerate()
    {
        let original = PathBuf::from(path);
        if crate::oplog::paths_equal(&original, Path::new(src)) {
            return Err(fail("来源与目标不能重叠。"));
        }
        let stored = tx
            .join("previous")
            .join(index.to_string())
            .join(original.file_name().unwrap());
        let digest = plan
            .expected_state
            .get(path)
            .ok_or_else(|| fail("缺少预览中的文件状态，请重新预览。"))?
            .clone();
        steps.push(MoveStep {
            digest,
            from: original,
            to: stored,
        });
    }
    steps.push(MoveStep {
        from: PathBuf::from(src),
        to: dest.to_path_buf(),
        digest: archive_digest.clone(),
    });
    steps.push(MoveStep {
        from: new_folder,
        to: folder.to_path_buf(),
        digest: folder_digest.clone(),
    });
    let manifest_path = tx.join("manifest.json");
    let record = record.map(|record| {
        let mut record = record.clone();
        record.archive_manifest = Some(manifest_path.to_string_lossy().to_string());
        record
    });
    let mut manifest = Manifest {
        id,
        phase: "applying".into(),
        latest,
        head,
        previous,
        completed: 0,
        steps,
        archive_dest: dest.to_path_buf(),
        folder_dest: folder.to_path_buf(),
        archive_digest,
        folder_digest,
        record,
        restored_state: Vec::new(),
    };
    write_json(&manifest_path, &manifest)?;
    let applied = (|| -> io::Result<()> {
        for i in 0..manifest.steps.len() {
            let step = &manifest.steps[i];
            relocate(&step.from, &step.to, &step.digest)?;
            manifest.completed = i + 1;
            write_json(&manifest_path, &manifest)?;
        }
        if latest {
            write_json(&manifest.head, &manifest.id)?;
        }
        manifest.phase = "committed".into();
        write_json(&manifest_path, &manifest)?;
        Ok(())
    })();
    if let Err(e) = applied {
        match rollback(&manifest) {
            Ok(()) => {
                if latest {
                    restore_head(&manifest)?;
                }
                manifest.phase = "aborted".into();
                write_json(&manifest_path, &manifest)?;
                let _ = fs::remove_dir_all(&tx);
                return Err(fail(format!("更新失败，已恢复原内容：{e}")));
            }
            Err(recovery) => {
                return Err(fail(format!(
                    "更新失败：{e}；恢复未完成：{recovery}；请保留 {}",
                    tx.display()
                )))
            }
        }
    }
    // Retire the prior operation even if moving its backup to Trash fails.
    let mut warning = None;
    if let Err(e) = retire_previous(&manifest, recycle) {
        warning = Some(format!("更新成功；旧备份清理失败，已保留：{e}"));
    }
    let _ = fs::remove_file(staged_zip);
    if let Some(record) = &mut manifest.record {
        record.warning = warning.clone();
        if let Err(e) = write_json(&manifest_path, &manifest) {
            warning = Some(format!("更新成功，附加状态写入失败：{e}"));
        }
    }
    Ok((manifest_path.to_string_lossy().to_string(), warning))
}

fn retire_previous(
    manifest: &Manifest,
    recycle: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    if let Some(previous) = &manifest.previous {
        if previous.exists() {
            no_links(previous)?;
            let mut old = read_manifest(previous)?;
            old.phase = "retired".into();
            write_json(previous, &old)?;
            recycle(previous.parent().unwrap())?;
        }
    }
    Ok(())
}

fn restore_head(manifest: &Manifest) -> io::Result<()> {
    if let Some(previous) = &manifest.previous {
        let old = read_manifest(previous)?;
        write_json(&manifest.head, &old.id)
    } else if manifest.head.exists() {
        fs::remove_file(&manifest.head)
    } else {
        Ok(())
    }
}

pub fn undo_reason(path: &str) -> Option<String> {
    let manifest = match read_manifest(Path::new(path)) {
        Ok(m) => m,
        Err(_) => return Some("撤销备份已清理或无法读取".into()),
    };
    if manifest.phase != "committed" {
        return Some("仅保留最近一次包更新的撤销能力".into());
    }
    if manifest.latest
        && read_head(&manifest.head).ok().flatten().as_deref() != Some(Path::new(path))
    {
        return Some("已被后续包更新替代".into());
    }
    if !manifest.archive_dest.exists() || !manifest.folder_dest.exists() {
        return Some("目标 ZIP 或解压目录已被后续操作移走".into());
    }
    None
}

pub fn undo(path: &str) -> io::Result<(Vec<PathBuf>, Vec<(String, u64, std::time::SystemTime)>)> {
    if let Some(reason) = undo_reason(path) {
        return Err(fail(reason));
    }
    let path = Path::new(path);
    let mut manifest = read_manifest(path)?;
    if fingerprint(&manifest.archive_dest)? != manifest.archive_digest
        || fingerprint(&manifest.folder_dest)? != manifest.folder_digest
    {
        return Err(fail("新 ZIP 或解压目录已被修改，请保留现状并手动处理。"));
    }
    // Check every restoration path and backup before changing anything.
    for step in &manifest.steps {
        if step.from.exists()
            && !crate::oplog::paths_equal(&step.from, &manifest.archive_dest)
            && !crate::oplog::paths_equal(&step.from, &manifest.folder_dest)
        {
            return Err(fail(format!("原位置已被占用：{}", step.from.display())));
        }
        if fingerprint(&step.to)? != step.digest {
            return Err(fail("撤销备份或文件已发生变化。"));
        }
    }
    // Reverse using a second journal, so failed undo can put the current version back.
    let reverse: Vec<_> = manifest
        .steps
        .iter()
        .rev()
        .map(|s| MoveStep {
            from: s.to.clone(),
            to: s.from.clone(),
            digest: s.digest.clone(),
        })
        .collect();
    let undo_path = path.with_file_name("undo.json");
    let mut journal = manifest.clone();
    journal.steps = reverse;
    journal.completed = 0;
    journal.phase = "applying".into();
    write_json(&undo_path, &journal)?;
    let result = (|| -> io::Result<()> {
        for i in 0..journal.steps.len() {
            let step = &journal.steps[i];
            relocate(&step.from, &step.to, &step.digest)?;
            journal.completed = i + 1;
            write_json(&undo_path, &journal)?;
        }
        let mut states = Vec::new();
        for path in restored_paths(&manifest) {
            capture_state(&path, &mut states)?;
        }
        manifest.restored_state = states;
        if manifest.latest && manifest.head.exists() {
            fs::remove_file(&manifest.head)?;
        }
        manifest.phase = "undone".into();
        write_json(path, &manifest)?;
        Ok(())
    })();
    if let Err(e) = result {
        rollback(&journal)
            .map_err(|r| fail(format!("撤销失败：{e}；恢复未完成：{r}，请保留备份。")))?;
        if manifest.latest {
            write_json(&manifest.head, &manifest.id)?;
        }
        let _ = fs::remove_file(undo_path);
        return Err(fail(format!("撤销失败，当前版本已恢复：{e}")));
    }
    let restored = restored_paths(&manifest);
    let _ = fs::remove_file(undo_path);
    // The generated extraction has returned into this operation's private folder.
    let generated = &manifest.steps.last().unwrap().from;
    if generated.exists() {
        let _ = fs::remove_dir_all(generated);
    }
    Ok((restored, manifest.restored_state))
}

fn capture_state(
    path: &Path,
    states: &mut Vec<(String, u64, std::time::SystemTime)>,
) -> io::Result<()> {
    no_links(path)?;
    let meta = fs::metadata(path)?;
    if meta.is_file() {
        states.push((
            path.to_string_lossy().to_string(),
            meta.len(),
            meta.modified()?,
        ));
    } else {
        for entry in fs::read_dir(path)? {
            capture_state(&entry?.path(), states)?;
        }
    }
    Ok(())
}

fn restored_paths(manifest: &Manifest) -> Vec<PathBuf> {
    manifest
        .steps
        .iter()
        .filter(|s| s.from != manifest.steps.last().unwrap().from)
        .map(|s| s.from.clone())
        .collect()
}

/// Group only matches already admitted by the rule and optional manual selection.
pub fn groups(files: Vec<FileMeta>) -> Vec<Vec<FileMeta>> {
    let mut groups: BTreeMap<String, Vec<FileMeta>> = BTreeMap::new();
    for meta in files {
        let stem = Path::new(&meta.name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        groups
            .entry(key(&package_name(&stem)))
            .or_default()
            .push(meta);
    }
    groups
        .into_values()
        .map(|mut files| {
            files.sort_by(|a, b| {
                b.modified
                    .cmp(&a.modified)
                    .then_with(|| a.path.cmp(&b.path))
            });
            files
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Action, Condition, Rule};
    use std::cell::RefCell;
    use std::time::{Duration, SystemTime};
    use zip::write::SimpleFileOptions;

    struct Fixture {
        root: PathBuf,
        downloads: PathBuf,
        target: PathBuf,
        recycled: RefCell<Vec<PathBuf>>,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("filepilot_archive_{}", uuid::Uuid::new_v4()));
            let downloads = root.join("downloads");
            let target = root.join("target");
            fs::create_dir_all(&downloads).unwrap();
            fs::create_dir(&target).unwrap();
            Self {
                root,
                downloads,
                target,
                recycled: RefCell::new(Vec::new()),
            }
        }
        fn zip(&self, name: &str, entries: &[(&str, &str)]) -> PathBuf {
            let path = self.downloads.join(name);
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            for (name, value) in entries {
                zip.start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
                zip.write_all(value.as_bytes()).unwrap();
            }
            zip.finish().unwrap().sync_all().unwrap();
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(60))
                .unwrap();
            path
        }
        fn plan(&self, path: &Path, retention: ArchiveRetention) -> ArchivePlan {
            plan(
                &FileMeta::from_path(path).unwrap(),
                self.target.to_str().unwrap(),
                &retention,
                vec![],
                &mut HashSet::new(),
            )
        }
        fn recycle(&self, path: &Path) -> io::Result<()> {
            let trash = self.root.join("trash");
            fs::create_dir_all(&trash)?;
            fs::rename(path, trash.join(path.file_name().unwrap()))?;
            self.recycled.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
        fn run(&self, path: &Path, retention: ArchiveRetention) -> io::Result<String> {
            execute_with_recycler(path.to_str().unwrap(), &self.plan(path, retention), &|p| {
                self.recycle(p)
            })
            .map(|v| v.0)
        }
        fn rule(&self) -> Rule {
            Rule {
                id: "zip".into(),
                name: "ZIP更新".into(),
                enabled: true,
                target_kind: crate::rules::TargetKind::File,
                watch_folders: vec![self.downloads.to_string_lossy().to_string()],
                conditions: vec![Condition::Extension {
                    exts: vec!["zip".into()],
                }],
                actions: vec![Action::MoveAndExtract {
                    dest: self.target.to_string_lossy().to_string(),
                    retention: ArchiveRetention::Latest,
                }],
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn archive_download_suffixes_group_without_merging_other_packages() {
        assert_eq!(package_name("filepilot (4)"), "filepilot");
        assert_eq!(package_name("filepilot(4)"), "filepilot");
        assert_eq!(package_name("filepilot(beta)"), "filepilot(beta)");
        assert_eq!(package_name("filepilot-debug"), "filepilot-debug");
    }

    #[test]
    fn archive_five_downloads_update_as_one_and_undo_restores_every_source() {
        let f = Fixture::new();
        let paths: Vec<_> = (0..5)
            .map(|i| {
                let p = f.zip(
                    &format!("filepilot({i}).zip"),
                    &[("app.exe", &format!("v{i}"))],
                );
                File::options()
                    .write(true)
                    .open(&p)
                    .unwrap()
                    .set_modified(SystemTime::now() - Duration::from_secs(100 - i))
                    .unwrap();
                p
            })
            .collect();
        let plans = crate::engine::dry_run(&f.rule(), &[f.downloads.clone()]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].src, paths[4].to_string_lossy());
        let plan = plans[0].archive.as_ref().unwrap();
        assert_eq!(plan.obsolete_sources.len(), 4);
        let manifest = execute_with_recycler(&plans[0].src, plan, &|p| f.recycle(p))
            .unwrap()
            .0;
        assert!(paths.iter().all(|p| !p.exists()));
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/app.exe")).unwrap(),
            "v4"
        );
        undo(&manifest).unwrap();
        assert!(paths.iter().all(|p| p.exists()));
        assert!(!f.target.join("filepilot").exists());
        assert!(!f.target.join("filepilot.zip").exists());
    }

    #[test]
    fn archive_existing_zip_and_directory_are_replaced_and_restored_together() {
        let f = Fixture::new();
        fs::write(f.target.join("filepilot.zip"), "oldzip").unwrap();
        fs::create_dir(f.target.join("filepilot")).unwrap();
        fs::write(f.target.join("filepilot/obsolete.dll"), "old").unwrap();
        fs::write(f.target.join("notes.txt"), "unrelated").unwrap();
        let src = f.zip("filepilot(4).zip", &[("app.exe", "new")]);
        let manifest = f.run(&src, ArchiveRetention::Latest).unwrap();
        assert!(!f.target.join("filepilot/obsolete.dll").exists());
        assert_eq!(
            fs::read_to_string(f.target.join("notes.txt")).unwrap(),
            "unrelated"
        );
        undo(&manifest).unwrap();
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot.zip")).unwrap(),
            "oldzip"
        );
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/obsolete.dll")).unwrap(),
            "old"
        );
        assert!(src.exists());
    }

    #[test]
    fn archive_keep_all_allocates_the_zip_and_folder_name_together() {
        let f = Fixture::new();
        fs::write(f.target.join("filepilot.zip"), "old").unwrap();
        fs::create_dir(f.target.join("filepilot (1)")).unwrap();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let preview = f.plan(&src, ArchiveRetention::All);
        assert!(preview.archive_dest.ends_with("filepilot (2).zip"));
        let manifest = f.run(&src, ArchiveRetention::All).unwrap();
        assert!(f.target.join("filepilot (2)/app.exe").exists());
        undo(&manifest).unwrap();
        assert!(src.exists());
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot.zip")).unwrap(),
            "old"
        );
    }

    #[test]
    fn archive_failed_new_zip_preserves_current_and_its_undo_backup() {
        let f = Fixture::new();
        let a = f.zip("filepilot.zip", &[("app.exe", "A")]);
        f.run(&a, ArchiveRetention::Latest).unwrap();
        let b = f.zip("filepilot.zip", &[("app.exe", "B")]);
        let undo_b = f.run(&b, ArchiveRetention::Latest).unwrap();
        let c = f.downloads.join("filepilot.zip");
        fs::write(&c, "broken ZIP").unwrap();
        assert!(f.run(&c, ArchiveRetention::Latest).is_err());
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/app.exe")).unwrap(),
            "B"
        );
        assert!(undo_reason(&undo_b).is_none());
        assert!(c.exists());
        assert_eq!(f.recycled.borrow().len(), 1); // only the initial A operation was retired by B
    }

    #[test]
    fn archive_next_success_recycles_older_backup_and_disables_older_undo() {
        let f = Fixture::new();
        let a = f.zip("filepilot.zip", &[("app.exe", "A")]);
        f.run(&a, ArchiveRetention::Latest).unwrap();
        let b = f.zip("filepilot.zip", &[("app.exe", "B")]);
        let undo_b = f.run(&b, ArchiveRetention::Latest).unwrap();
        let c = f.zip("filepilot.zip", &[("app.exe", "C")]);
        let undo_c = f.run(&c, ArchiveRetention::Latest).unwrap();
        assert!(undo_reason(&undo_b).is_some());
        assert!(undo_reason(&undo_c).is_none());
        undo(&undo_c).unwrap();
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/app.exe")).unwrap(),
            "B"
        );
        assert!(c.exists());
        assert!(undo_reason(&undo_b).is_some());
    }

    #[test]
    fn archive_restart_finishes_older_backup_retirement() {
        let f = Fixture::new();
        let a = f.zip("filepilot.zip", &[("app.exe", "A")]);
        let undo_a = f.run(&a, ArchiveRetention::Latest).unwrap();
        let b = f.zip("filepilot.zip", &[("app.exe", "B")]);
        let record = crate::oplog::OpRecord::new(
            "batch",
            "zip",
            "ZIP更新",
            "move_extract".into(),
            b.to_string_lossy().to_string(),
            None,
            None,
        );
        let (undo_b, _) = execute_with_context(
            b.to_str().unwrap(),
            &f.plan(&b, ArchiveRetention::Latest),
            &|_| Err(fail("模拟清理前退出")),
            Some(&record),
        )
        .unwrap();
        let mut old = read_manifest(Path::new(&undo_a)).unwrap();
        old.phase = "committed".into();
        write_json(Path::new(&undo_a), &old).unwrap();
        let recovered = recover_with_recycler(&f.target, &|p| f.recycle(p)).unwrap();
        assert!(!Path::new(&undo_a).exists());
        assert_eq!(f.recycled.borrow().len(), 1);
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/app.exe")).unwrap(),
            "B"
        );
        assert!(recovered
            .iter()
            .find(|r| r.id == record.id)
            .unwrap()
            .warning
            .is_none());
        assert!(recover_with_recycler(&f.target, &|p| f.recycle(p)).is_ok());
        assert_eq!(f.recycled.borrow().len(), 1);
        undo(&undo_b).unwrap();
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/app.exe")).unwrap(),
            "A"
        );
    }

    #[test]
    fn archive_recycler_failure_is_a_warning_after_successful_update() {
        let f = Fixture::new();
        let a = f.zip("filepilot.zip", &[("app.exe", "A")]);
        let undo_a = f.run(&a, ArchiveRetention::Latest).unwrap();
        let b = f.zip("filepilot.zip", &[("app.exe", "B")]);
        let (undo_b, warning) = execute_with_recycler(
            b.to_str().unwrap(),
            &f.plan(&b, ArchiveRetention::Latest),
            &|_| Err(fail("回收站不可用")),
        )
        .unwrap();
        assert!(warning.is_some());
        assert!(Path::new(&undo_a).exists());
        assert!(undo_reason(&undo_a).is_some());
        undo(&undo_b).unwrap();
    }

    #[test]
    fn archive_dirty_extraction_refuses_undo_before_restoring_anything() {
        let f = Fixture::new();
        fs::write(f.target.join("filepilot.zip"), "old").unwrap();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let manifest = f.run(&src, ArchiveRetention::Latest).unwrap();
        fs::write(f.target.join("filepilot/user.txt"), "keep me").unwrap();
        assert!(undo(&manifest).is_err());
        assert!(!src.exists());
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/user.txt")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn archive_dirty_zip_and_occupied_source_refuse_undo() {
        let f = Fixture::new();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let manifest = f.run(&src, ArchiveRetention::Latest).unwrap();
        fs::write(&src, "new download").unwrap();
        assert!(undo(&manifest).is_err());
        fs::remove_file(&src).unwrap();
        fs::write(f.target.join("filepilot.zip"), "modified").unwrap();
        assert!(undo(&manifest).is_err());
        assert!(f.target.join("filepilot/app.exe").exists());
    }

    #[test]
    fn archive_traversal_and_windows_unsafe_entries_are_rejected() {
        for name in [
            "../outside.txt",
            "C:/outside.txt",
            "folder/../outside.txt",
            "folder/CON.txt",
            "folder/file:stream",
            "folder/trailing.",
        ] {
            let f = Fixture::new();
            let src = f.zip("filepilot.zip", &[(name, "bad")]);
            assert!(f.run(&src, ArchiveRetention::Latest).is_err(), "{name}");
            assert!(src.exists());
            assert!(!f.target.join("filepilot.zip").exists());
        }
    }

    #[test]
    fn archive_case_collisions_and_corrupt_crc_are_rejected() {
        let f = Fixture::new();
        let src = f.zip("filepilot.zip", &[("A.txt", "a"), ("a.txt", "b")]);
        assert!(f.run(&src, ArchiveRetention::Latest).is_err());
        let src = f.zip("filepilot.zip", &[("app.exe", "unique-payload")]);
        let mut data = fs::read(&src).unwrap();
        let offset = data
            .windows(14)
            .position(|s| s == b"unique-payload")
            .unwrap();
        data[offset] ^= 1;
        fs::write(&src, data).unwrap();
        assert!(f.run(&src, ArchiveRetention::Latest).is_err());
        assert!(src.exists());
    }

    #[test]
    fn archive_preview_detects_old_directory_changes() {
        let f = Fixture::new();
        fs::create_dir(f.target.join("filepilot")).unwrap();
        fs::write(f.target.join("filepilot/app.exe"), "old").unwrap();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let plan = f.plan(&src, ArchiveRetention::Latest);
        fs::write(f.target.join("filepilot/added.txt"), "keep").unwrap();
        assert!(execute_with_recycler(src.to_str().unwrap(), &plan, &|p| f.recycle(p)).is_err());
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/added.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn archive_manual_selection_and_watch_skip_happen_before_grouping() {
        let f = Fixture::new();
        let old = f.zip("filepilot.zip", &[("app.exe", "old")]);
        let rule = f.rule();
        let preview = crate::engine::dry_run(&rule, &[f.downloads.clone()]);
        let newer = f.zip("filepilot (1).zip", &[("app.exe", "new")]);
        let selected =
            crate::engine::dry_run_filtered(&rule, &[f.downloads.clone()], &|p| p == old);
        assert_eq!(selected[0].src, preview[0].src);
        assert!(selected[0]
            .archive
            .as_ref()
            .unwrap()
            .obsolete_sources
            .is_empty());
        let skipped =
            crate::engine::dry_run_filtered(&rule, &[f.downloads.clone()], &|p| p != newer);
        assert_eq!(skipped[0].src, old.to_string_lossy());
    }

    #[test]
    fn archive_ambiguous_timestamps_block_latest_but_not_keep_all() {
        let f = Fixture::new();
        let a = f.zip("filepilot.zip", &[("app.exe", "A")]);
        let b = f.zip("filepilot(4).zip", &[("app.exe", "B")]);
        let time = SystemTime::now() - Duration::from_secs(60);
        for path in [&a, &b] {
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        }
        let plans = crate::engine::dry_run(&f.rule(), &[f.downloads.clone()]);
        assert!(plans[0]
            .archive
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("修改时间相同"));
        let mut rule = f.rule();
        rule.actions = vec![Action::MoveAndExtract {
            dest: f.target.to_string_lossy().to_string(),
            retention: ArchiveRetention::All,
        }];
        assert!(crate::engine::dry_run(&rule, &[f.downloads.clone()])
            .iter()
            .all(|p| p.archive.as_ref().unwrap().error.is_none()));
    }

    #[test]
    fn archive_restart_recovers_completed_update_missing_jsonl_and_undo_state() {
        let f = Fixture::new();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let plan = f.plan(&src, ArchiveRetention::Latest);
        let record = crate::oplog::OpRecord::new(
            "batch",
            "zip",
            "ZIP更新",
            "move_extract".into(),
            src.to_string_lossy().to_string(),
            Some(plan.folder_dest.clone()),
            None,
        );
        let path = execute_with_context(
            src.to_str().unwrap(),
            &plan,
            &|p| f.recycle(p),
            Some(&record),
        )
        .unwrap()
        .0;
        let recovered = recover(&f.target).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].id, record.id);
        let log = f.root.join("log.jsonl");
        crate::oplog::reconcile_archive_records(&log, &recovered).unwrap();
        crate::oplog::reconcile_archive_records(&log, &recovered).unwrap();
        assert_eq!(crate::oplog::read_all(&log).len(), 1);
        let (_, states) = undo(&path).unwrap();
        File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(2))
            .unwrap();
        let after = recover(&f.target).unwrap();
        assert!(after[0].undone);
        assert_eq!(
            after[0].restored_state, states,
            "restart must not re-protect a changed download using fresh metadata"
        );
        crate::oplog::reconcile_archive_records(&log, &after).unwrap();
        assert!(crate::oplog::read_all(&log)[0].undone);
    }

    #[test]
    fn archive_restart_rolls_back_interrupted_update() {
        let f = Fixture::new();
        fs::write(f.target.join("filepilot.zip"), "old").unwrap();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let path = f.run(&src, ArchiveRetention::Latest).unwrap();
        let path = Path::new(&path);
        let mut manifest = read_manifest(path).unwrap();
        manifest.phase = "applying".into();
        write_json(path, &manifest).unwrap();
        recover(&f.target).unwrap();
        assert!(src.exists());
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot.zip")).unwrap(),
            "old"
        );
        assert!(!f.target.join("filepilot").exists());
        assert_eq!(read_manifest(path).unwrap().phase, "aborted");
    }

    #[test]
    fn archive_restart_rolls_back_interrupted_undo_inflight_move() {
        let f = Fixture::new();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let path = f.run(&src, ArchiveRetention::Latest).unwrap();
        let path = Path::new(&path);
        let mut journal = read_manifest(path).unwrap();
        journal.steps = journal
            .steps
            .iter()
            .rev()
            .map(|s| MoveStep {
                from: s.to.clone(),
                to: s.from.clone(),
                digest: s.digest.clone(),
            })
            .collect();
        journal.completed = 0;
        journal.phase = "applying".into();
        let undo_path = path.with_file_name("undo.json");
        write_json(&undo_path, &journal).unwrap();
        let step = &journal.steps[0];
        relocate(&step.from, &step.to, &step.digest).unwrap();
        recover(&f.target).unwrap();
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/app.exe")).unwrap(),
            "new"
        );
        assert!(!src.exists());
        assert!(!undo_path.exists());
        assert!(undo_reason(path.to_str().unwrap()).is_none());
    }

    #[test]
    fn archive_malformed_manifest_paths_are_rejected_before_moving_files() {
        let f = Fixture::new();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let path = f.run(&src, ArchiveRetention::Latest).unwrap();
        let mut manifest = read_manifest(Path::new(&path)).unwrap();
        manifest.folder_dest = f.root.join("unrelated");
        write_json(Path::new(&path), &manifest).unwrap();
        assert!(undo(&path).is_err());
        assert!(f.target.join("filepilot/app.exe").exists());
    }

    #[test]
    fn archive_fresh_download_is_processed_after_monitor_debounce() {
        let f = Fixture::new();
        let rule = f.rule();
        let watched = f.downloads.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = crate::watcher::start_watching(
            vec![watched.clone()],
            Box::new(move |folder| {
                let plan = crate::engine::dry_run(&rule, &[folder.to_path_buf()]);
                let records = crate::engine::apply_plan(&plan, "watch", &rule.id);
                let count = records.iter().filter(|r| r.error.is_none()).count();
                if count > 0 {
                    let _ = tx.send(count);
                }
                count
            }),
        );
        // start_watching registers its native watcher on a worker thread.
        std::thread::sleep(Duration::from_millis(500));
        let src = f.zip("filepilot.zip", &[("app.exe", "fresh")]);
        File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(SystemTime::now())
            .unwrap();
        let result = rx.recv_timeout(Duration::from_secs(15));
        handle.shutdown();
        assert_eq!(result.unwrap(), 1);
        assert!(!src.exists());
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/app.exe")).unwrap(),
            "fresh"
        );
    }

    #[cfg(windows)]
    #[test]
    fn archive_case_insensitive_targets_undo_as_one_package() {
        let f = Fixture::new();
        fs::write(f.target.join("FilePilot.zip"), "old").unwrap();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        let path = f.run(&src, ArchiveRetention::Latest).unwrap();
        undo(&path).unwrap();
        assert_eq!(
            fs::read_to_string(f.target.join("FilePilot.zip")).unwrap(),
            "old"
        );
    }

    #[cfg(windows)]
    #[test]
    fn archive_locked_old_folder_rolls_back_zip_move() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        fs::write(f.target.join("filepilot.zip"), "oldzip").unwrap();
        fs::create_dir(f.target.join("filepilot")).unwrap();
        fs::write(f.target.join("filepilot/app.exe"), "old").unwrap();
        let locked = File::options()
            .read(true)
            .share_mode(1)
            .open(f.target.join("filepilot/app.exe"))
            .unwrap();
        let src = f.zip("filepilot.zip", &[("app.exe", "new")]);
        assert!(f.run(&src, ArchiveRetention::Latest).is_err());
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot.zip")).unwrap(),
            "oldzip"
        );
        assert!(src.exists());
        drop(locked);
    }

    #[cfg(windows)]
    #[test]
    fn archive_open_child_directory_reports_the_blocking_path() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let first = f.zip("filepilot.zip", &[("msi/setup.msi", "old")]);
        let previous = f.run(&first, ArchiveRetention::Latest).unwrap();
        let child = f.target.join("filepilot/msi");
        // A directory viewer may allow DELETE sharing. An open descendant still
        // prevents Windows from moving its parent tree, even with all files closed.
        let opened = File::options()
            .read(true)
            .share_mode(7)
            .custom_flags(0x02000000)
            .open(&child)
            .unwrap();
        let source = f.zip("filepilot(1).zip", &[("msi/setup.msi", "new")]);
        let error = f
            .run(&source, ArchiveRetention::Latest)
            .unwrap_err()
            .to_string();
        eprintln!("open descendant directory: {error}");
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/msi/setup.msi")).unwrap(),
            "old"
        );
        assert!(source.exists());
        assert!(undo_reason(&previous).is_none());
        assert!(f.recycled.borrow().is_empty());
        let reported = error
            .split_once("文件夹被占用或无法访问：")
            .expect("错误应标明目录占用")
            .1
            .split('（')
            .next()
            .unwrap();
        assert!(
            crate::oplog::paths_equal(Path::new(reported), &child),
            "错误应指向被占用的子目录：{error}"
        );
        drop(opened);
        let current = f.run(&source, ArchiveRetention::Latest).unwrap();
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/msi/setup.msi")).unwrap(),
            "new"
        );
        undo(&current).unwrap();
        assert_eq!(
            fs::read_to_string(f.target.join("filepilot/msi/setup.msi")).unwrap(),
            "old"
        );
        assert!(source.exists());
    }
}
