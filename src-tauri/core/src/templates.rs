use crate::rules::{Action, DateSource, FileMeta, FileOperation};
use chrono::{DateTime, Local};
use std::path::{Component, Path};
use std::time::SystemTime;

pub fn render(pattern: &str, meta: &FileMeta, source: DateSource) -> Result<String, String> {
    render_at(pattern, meta, source, SystemTime::now())
}
pub fn render_at(
    pattern: &str,
    meta: &FileMeta,
    source: DateSource,
    at: SystemTime,
) -> Result<String, String> {
    let time: DateTime<Local> = match source {
        DateSource::Now => at.into(),
        DateSource::Created => meta.created.ok_or("无法读取创建时间，未执行。")?.into(),
        DateSource::Modified => meta.modified.ok_or("无法读取修改时间，未执行。")?.into(),
    };
    let name = if meta.is_directory {
        meta.name.clone()
    } else {
        Path::new(&meta.name)
            .file_stem()
            .map(|v| v.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    // Parse placeholders before substitution; braces in an actual filename remain literal.
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..].find('}').ok_or("模板括号不完整。")? + start;
        out.push_str(&match &rest[start + 1..end] {
            "name" => name.clone(),
            "ext" => meta.ext.clone(),
            "date" => time.format("%Y-%m-%d").to_string(),
            "datetime" => time.format("%Y%m%d-%H%M%S").to_string(),
            "year" => time.format("%Y").to_string(),
            "month" => time.format("%m").to_string(),
            "day" => time.format("%d").to_string(),
            _ => {
                return Err(
                    "未知模板变量；支持 name、ext、date、datetime、year、month、day。".into(),
                )
            }
        });
        rest = &rest[end + 1..];
    }
    if rest.contains('}') {
        return Err("模板括号不完整。".into());
    }
    out.push_str(rest);
    Ok(out)
}
pub fn resolve(action: &Action, meta: &FileMeta) -> Result<Action, String> {
    resolve_at(action, meta, SystemTime::now())
}
pub fn resolve_at(action: &Action, meta: &FileMeta, at: SystemTime) -> Result<Action, String> {
    Ok(match action {
        Action::RenameWithDate {
            pattern,
            date_source,
        } => Action::Rename {
            pattern: render_at(pattern, meta, *date_source, at)?,
        },
        Action::Rename { pattern } => Action::Rename {
            pattern: render_at(pattern, meta, DateSource::Now, at)?,
        },
        Action::Organize {
            operation,
            dest,
            subfolder,
            date_source,
        } => {
            let subfolder = render_at(subfolder, meta, *date_source, at)?.replace('\\', "/");
            let path = Path::new(&subfolder);
            if !subfolder.is_empty()
                && (path
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
                    || subfolder
                        .split('/')
                        .any(|part| !crate::archive::valid_component(part)))
            {
                return Err("子文件夹必须是相对路径，不能包含 .. 或非法名称。".into());
            }
            let dest = Path::new(dest).join(path).to_string_lossy().to_string();
            match operation {
                FileOperation::Move => Action::Move { dest },
                FileOperation::Copy => Action::Copy { dest },
            }
        }
        _ => action.clone(),
    })
}
