//! Compile the typed editor model to standard JSONLogic. No user scripts or I/O.
use crate::rules::{Condition, DateOperator, FileDate, FileMeta, MatchMode, NameOperator};
use datalogic_rs::{Engine, Logic};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::SystemTime;

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(Engine::new)
}
pub struct Matcher {
    logic: Logic,
}
impl Matcher {
    pub fn new(conditions: &[Condition]) -> Result<Self, String> {
        let expression = group(conditions, MatchMode::All, false, 0)?;
        engine()
            .compile(expression.to_string().as_str())
            .map(|logic| Self { logic })
            .map_err(|e| e.to_string())
    }
    pub fn matches(&self, meta: &FileMeta) -> bool {
        let age =
            |time: Option<SystemTime>| time.and_then(|t| t.elapsed().ok()).map(|age| age.as_secs());
        let data=json!({"name":meta.name.to_lowercase(),"ext":meta.ext.to_lowercase(),"size":meta.size,"is_file":!meta.is_directory,"created_age":age(meta.created),"modified_age":age(meta.modified)}).to_string();
        let arena = datalogic_rs::bumpalo::Bump::new();
        engine()
            .evaluate(&self.logic, data.as_str(), &arena)
            .map(|value| value.as_bool() == Some(true))
            .unwrap_or(false)
    }
}
fn group(
    conditions: &[Condition],
    mode: MatchMode,
    negate: bool,
    depth: usize,
) -> Result<Value, String> {
    if conditions.is_empty() || conditions.len() > 100 || depth > 4 {
        return Err("条件组不能为空，最多嵌套 4 层，每组最多 100 个条件。".into());
    }
    let children = conditions
        .iter()
        .map(|c| expression(c, depth + 1))
        .collect::<Result<Vec<_>, _>>()?;
    let expr = if mode == MatchMode::All {
        json!({"and":children})
    } else {
        json!({"or":children})
    };
    if negate {
        let mut fields = Vec::new();
        required_dates(conditions, &mut fields);
        let mut terms:Vec<Value>=fields.into_iter().map(|field|json!({"!==":[{"var":if field==FileDate::Created {"created_age"} else {"modified_age"}},null]})).collect();
        terms.push(json!({"!":expr}));
        Ok(json!({"and":terms}))
    } else {
        Ok(expr)
    }
}

fn required_dates(conditions: &[Condition], fields: &mut Vec<FileDate>) {
    for condition in conditions {
        match condition {
            Condition::CreatedWithinDays { .. } => {
                if !fields.contains(&FileDate::Created) {
                    fields.push(FileDate::Created);
                }
            }
            Condition::DateMatch { field, .. } => {
                if !fields.contains(field) {
                    fields.push(*field);
                }
            }
            Condition::Group { conditions, .. } => required_dates(conditions, fields),
            _ => (),
        }
    }
}
fn expression(condition: &Condition, depth: usize) -> Result<Value, String> {
    let name = json!({"var":"name"});
    let size = json!({"var":"size"});
    let file = json!({"var":"is_file"});
    let keyword = |text: &str| -> Result<String, String> {
        if text.trim().is_empty() {
            Err("名称关键词不能为空。".into())
        } else {
            Ok(text.to_lowercase())
        }
    };
    let date = |field: FileDate, operator: DateOperator, days: u64| -> Result<Value, String> {
        let seconds = days
            .checked_mul(86400)
            .filter(|_| days > 0 && days <= 365000)
            .ok_or("天数须为 1 至 365000。")?;
        let age = json!({"var":if field==FileDate::Created {"created_age"}else{"modified_age"}});
        Ok(
            json!({"and":[{"!==":[age.clone(),null]},if operator==DateOperator::WithinDays {json!({"<":[age,seconds]})}else{json!({">=":[age,seconds]})}]}),
        )
    };
    Ok(match condition {
        Condition::Extension { exts } => {
            let exts: Vec<_> = exts
                .iter()
                .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if exts.is_empty() {
                return Err("扩展名不能为空。".into());
            }
            json!({"and":[file,{"in":[{"var":"ext"},exts]}]})
        }
        Condition::NameContains { keyword: text } => json!({"in":[keyword(text)?,name]}),
        Condition::NameMatch {
            operator,
            keyword: text,
        } => {
            let text = keyword(text)?;
            match operator {
                NameOperator::Contains => json!({"in":[text,name]}),
                NameOperator::NotContains => json!({"!":{"in":[text,name]}}),
                NameOperator::Equals => json!({"===":[name,text]}),
                NameOperator::StartsWith => {
                    json!({"===":[{"substr":[name,0,text.chars().count()]},text]})
                }
                NameOperator::EndsWith => {
                    json!({"===":[{"substr":[name,-(text.chars().count() as i64)]},text]})
                }
            }
        }
        Condition::SizeGreaterThan { bytes } => json!({"and":[file,{">":[size,bytes]}]}),
        Condition::SizeLessThan { bytes } => json!({"and":[file,{"<":[size,bytes]}]}),
        Condition::SizeBetween { min, max } => {
            if min > max {
                return Err("大小区间的下限不能大于上限。".into());
            }
            json!({"and":[file,{"<=":[min,size,max]}]})
        }
        Condition::CreatedWithinDays { days } => {
            date(FileDate::Created, DateOperator::WithinDays, *days)?
        }
        Condition::DateMatch {
            field,
            operator,
            days,
        } => date(*field, *operator, *days)?,
        Condition::Group {
            mode,
            negate,
            conditions,
        } => group(conditions, *mode, *negate, depth)?,
    })
}
