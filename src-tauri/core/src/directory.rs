//! Whole-directory operations. No merging, overwriting or following links.
use crate::{archive, oplog};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub fn settled(path: &Path, quiet: Duration) -> bool {
    fn visit(path: &Path, quiet: Duration) -> io::Result<bool> {
        archive::no_links(path)?;
        let md = fs::metadata(path)?;
        if md.modified()?.elapsed().map(|d| d <= quiet).unwrap_or(true) {
            return Ok(false);
        }
        if md.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if crate::engine::TEMP_SUFFIXES
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                    || !visit(&entry.path(), quiet)?
                {
                    return Ok(false);
                }
            }
        } else if !md.is_file() {
            return Ok(false);
        }
        Ok(true)
    }
    visit(path, quiet).unwrap_or(false)
}

pub fn validate_destination(source: &Path, destination: &Path) -> io::Result<()> {
    archive::no_links(source)?;
    archive::no_links(destination)?;
    if !source.is_absolute() || !destination.is_absolute() {
        return Err(io::Error::other("请选择完整的文件夹路径。"));
    }
    if destination
        .ancestors()
        .any(|p| oplog::paths_equal(p, source))
    {
        return Err(io::Error::other("不能把文件夹放入自身或它的子文件夹。"));
    }
    if !archive::valid_component(
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
    ) {
        return Err(io::Error::other("目标文件夹名称无效。"));
    }
    Ok(())
}

pub fn unique_destination(destination: &Path) -> PathBuf {
    if !destination.exists() {
        return destination.to_path_buf();
    }
    let parent = destination.parent().unwrap();
    let name = destination.file_name().unwrap().to_string_lossy();
    for i in 1.. {
        let candidate = parent.join(format!("{name} ({i})"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn move_to(source: &Path, destination: &Path, expected: &str) -> io::Result<()> {
    validate_destination(source, destination)?;
    // An atomic rename leaves the source intact on failure. Cross-volume moves
    // require a durable copy/delete journal and are intentionally not enabled yet.
    archive::relocate(source, destination, expected).map_err(|e| {
        io::Error::other(format!(
            "文件夹移动失败：{e}。目前支持同盘移动；跨盘请使用复制。"
        ))
    })
}

struct StagingDirectory(PathBuf);
impl Drop for StagingDirectory {
    fn drop(&mut self) {
        // This uniquely-created directory only contains this operation's copies.
        if archive::no_links(&self.0).is_ok() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

pub fn copy_to(source: &Path, destination: &Path, expected: &str) -> io::Result<()> {
    validate_destination(source, destination)?;
    if archive::fingerprint(source)? != expected {
        return Err(io::Error::other("来源文件夹已变化，请重新预览。"));
    }
    let parent = destination.parent().unwrap();
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".filepilot-copy-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let _cleanup = StagingDirectory(staging.clone());
    copy_contents(source, &staging)?;
    if archive::fingerprint(source)? != expected
        || archive::fingerprint(&staging)? != expected
        || !settled(source, Duration::from_secs(5))
    {
        return Err(io::Error::other(
            "复制过程中来源内容已变化，原文件夹保留，请重新预览。",
        ));
    }
    archive::relocate(&staging, destination, expected)
}

fn copy_contents(source: &Path, destination: &Path) -> io::Result<()> {
    archive::no_links(source)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        archive::no_links(&from)?;
        let to = destination.join(entry.file_name());
        let md = fs::metadata(&from)?;
        if md.is_dir() {
            fs::create_dir(&to)?;
            copy_contents(&from, &to)?;
        } else if md.is_file() {
            // Reserve names within our private tree, then use the OS copy
            // primitive to preserve Windows attributes and alternate streams.
            drop(File::options().create_new(true).write(true).open(&to)?);
            fs::copy(&from, &to)?;
            let mut options = File::options();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.access_mode(0x100); // FILE_WRITE_ATTRIBUTES, including readonly files
            }
            options.open(&to)?.set_modified(md.modified()?)?;
        } else {
            return Err(io::Error::other("文件夹包含不支持的特殊文件。"));
        }
    }
    Ok(())
}

/// A restored directory is protected until any nested entry changes.
/// Metadata avoids re-reading every file's contents on every watcher callback.
pub fn state(path: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    fn visit(path: &Path, hash: &mut Sha256) -> io::Result<()> {
        archive::no_links(path)?;
        let md = fs::metadata(path)?;
        hash.update(if md.is_dir() { b"d" } else { b"f" });
        hash.update(md.len().to_le_bytes());
        let time = md
            .modified()?
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(io::Error::other)?;
        hash.update(time.as_nanos().to_le_bytes());
        if md.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(path)?.collect::<Result<_, _>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                hash.update(entry.file_name().to_string_lossy().as_bytes());
                hash.update([0]);
                visit(&entry.path(), hash)?;
            }
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    visit(path, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}
