//! Compile the typed editor model to standard JSONLogic. No user scripts or I/O.
//! Regex conditions run in Rust (linear-time engine); JSONLogic only sees their boolean result.
use crate::rules::{Condition, DateOperator, FileDate, FileMeta, MatchMode, NameOperator};
use datalogic_rs::{Engine, Logic};
use regex::{Regex, RegexBuilder};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::SystemTime;

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(Engine::new)
}
/// 与模板内置变量冲突的捕获组名
const RESERVED: &[&str] = &["name", "ext", "date", "datetime", "year", "month", "day"];

pub fn compile_regex(pattern: &str) -> Result<Regex, String> {
    if pattern.trim().is_empty() {
        return Err("正则表达式不能为空。".into());
    }
    if pattern.chars().count() > 500 {
        return Err("正则表达式最多 500 个字符。".into());
    }
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(true)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|e| {
            format!(
                "正则表达式无效：{}",
                e.to_string().lines().last().unwrap_or_default().trim()
            )
        })?;
    if let Some(name) = regex
        .capture_names()
        .flatten()
        .find(|name| RESERVED.contains(name))
    {
        return Err(format!("捕获组名「{name}」与内置变量重名，请换一个名称。"));
    }
    Ok(regex)
}

/// 可供模板引用的捕获组名：排除组内的正则匹配时条件不成立，不提供变量。
pub fn capture_names(conditions: &[Condition]) -> Vec<String> {
    fn walk(conditions: &[Condition], out: &mut Vec<String>) {
        for condition in conditions {
            match condition {
                Condition::NameMatch {
                    operator: NameOperator::Regex,
                    keyword,
                } => {
                    if let Ok(regex) = compile_regex(keyword) {
                        for name in regex.capture_names().flatten() {
                            if !out.iter().any(|n| n == name) {
                                out.push(name.into());
                            }
                        }
                    }
                }
                Condition::Group {
                    negate: false,
                    conditions,
                    ..
                } => walk(conditions, out),
                _ => (),
            }
        }
    }
    let mut out = Vec::new();
    walk(conditions, &mut out);
    out
}

struct Compiler {
    /// (正则, 是否提供捕获变量)
    regexes: Vec<(Regex, bool)>,
}

pub struct Matcher {
    logic: Logic,
    regexes: Vec<(Regex, bool)>,
}
impl Matcher {
    pub fn new(conditions: &[Condition]) -> Result<Self, String> {
        let mut compiler = Compiler {
            regexes: Vec::new(),
        };
        let expression = group(conditions, MatchMode::All, false, 0, false, &mut compiler)?;
        engine()
            .compile(expression.to_string().as_str())
            .map(|logic| Self {
                logic,
                regexes: compiler.regexes,
            })
            .map_err(|e| e.to_string())
    }
    /// 按条件顺序收集捕获值；同名变量取第一个非空值。
    pub fn captures(&self, meta: &FileMeta) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for (regex, capturing) in &self.regexes {
            if !capturing {
                continue;
            }
            if let Some(found) = regex.captures(&meta.name) {
                for name in regex.capture_names().flatten() {
                    if let Some(value) = found.name(name).map(|m| m.as_str().trim()) {
                        if !value.is_empty() {
                            out.entry(name.to_string())
                                .or_insert_with(|| value.to_string());
                        }
                    }
                }
            }
        }
        out
    }
    pub fn matches(&self, meta: &FileMeta) -> bool {
        let age =
            |time: Option<SystemTime>| time.and_then(|t| t.elapsed().ok()).map(|age| age.as_secs());
        let mut data=json!({"name":meta.name.to_lowercase(),"ext":meta.ext.to_lowercase(),"size":meta.size,"is_file":!meta.is_directory,"created_age":age(meta.created),"modified_age":age(meta.modified)});
        for (index, (regex, _)) in self.regexes.iter().enumerate() {
            data[format!("re_{index}")] = json!(regex.is_match(&meta.name));
        }
        let data = data.to_string();
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
    negated: bool,
    compiler: &mut Compiler,
) -> Result<Value, String> {
    if conditions.is_empty() || conditions.len() > 100 || depth > 4 {
        return Err("条件组不能为空，最多嵌套 4 层，每组最多 100 个条件。".into());
    }
    let children = conditions
        .iter()
        .map(|c| expression(c, depth + 1, negated || negate, compiler))
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
fn expression(
    condition: &Condition,
    depth: usize,
    negated: bool,
    compiler: &mut Compiler,
) -> Result<Value, String> {
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
            operator: NameOperator::Regex,
            keyword: pattern,
        } => {
            compiler.regexes.push((compile_regex(pattern)?, !negated));
            json!({"var":format!("re_{}", compiler.regexes.len() - 1)})
        }
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
                NameOperator::Regex => unreachable!(),
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
        } => group(conditions, *mode, *negate, depth, negated, compiler)?,
    })
}
