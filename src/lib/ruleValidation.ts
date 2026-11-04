import type { Condition, Rule } from "./types";

const BUILTIN_VARIABLES = ["name", "ext", "date", "datetime", "year", "month", "day"];

/** 正则中的命名捕获组，支持 (?<名>…) 与 (?P<名>…)。语法完整校验由后端保存时完成。 */
export function regexGroupNames(pattern: string): string[] {
  return [...pattern.matchAll(/(?<!\\)\(\?P?<([^>=!][^>]*)>/g)].map((match) => match[1]);
}

/** 模板可用的捕获变量：排除组内的正则不提供变量。 */
export function captureVariables(conditions: Condition[]): string[] {
  return [...new Set(conditions.flatMap((condition) =>
    condition.type === "group" ? (condition.negate ? [] : captureVariables(condition.conditions))
      : condition.type === "name_match" && condition.operator === "regex" ? regexGroupNames(condition.keyword) : []))];
}

export function validateRule(rule: Rule): Record<string, string> {
  const errors: Record<string, string> = {};
  if (!rule.name.trim()) errors.name = "请给规则起个名字。";
  if (!rule.watch_folders.length) errors["source-0"] = "请选择来源文件夹。";
  rule.watch_folders.forEach((folder, index) => {
    if (!folder.trim()) errors[`source-${index}`] = rule.watch_folders.length > 1 ? "请选择来源文件夹，或移除这一行。" : "请选择来源文件夹。";
  });
  if (!rule.conditions.length) errors.conditions = "请至少添加一个匹配条件。";
  const validateConditions=(conditions:Condition[],prefix="condition",depth=0)=>conditions.forEach((condition,index)=>{
    const key=`${prefix}-${index}`;
    if(condition.type === "group") {
      if(!condition.conditions.length) errors[key]="条件组不能为空。";
      if(depth>=4 || condition.conditions.length>100) errors[key]="条件组最多嵌套 4 层，每组最多 100 个条件。";
      validateConditions(condition.conditions,key,depth+1);return;
    }
    if (condition.type === "extension" && !condition.exts.some((ext) => ext.trim().replace(/^\.+/, ""))) errors[key] = "请至少选择一种扩展名。";
    if (condition.type === "name_contains" && !condition.keyword.trim()) errors[key] = rule.target_kind === "directory" ? "请输入文件夹名关键词。" : "请输入文件名关键词。";
    if (condition.type === "name_match" && !condition.keyword.trim()) errors[key]=condition.operator === "regex" ? "请输入正则表达式。" : "请输入名称关键词。";
    if (condition.type === "name_match" && condition.operator === "regex" && condition.keyword.trim()) {
      const reserved=regexGroupNames(condition.keyword).find((name)=>BUILTIN_VARIABLES.includes(name));
      if ([...condition.keyword].length>500) errors[key]="正则表达式最多 500 个字符。";
      else if (reserved) errors[key]=`捕获组名「${reserved}」与内置变量重名，请换一个名称。`;
    }
    if (condition.type === "size_greater_than" && (!Number.isSafeInteger(condition.bytes) || condition.bytes <= 0)) errors[key] = "文件大小必须大于 0。";
    if (condition.type === "created_within_days" && (!Number.isInteger(condition.days) || condition.days < 1 || condition.days>365000)) errors[key] = "创建天数须为 1 至 365000 的整数。";
    if (condition.type === "size_less_than" && (!Number.isSafeInteger(condition.bytes) || condition.bytes <= 0)) errors[key]="文件大小必须大于 0。";
    if (condition.type === "size_between" && (![condition.min,condition.max].every(v=>Number.isSafeInteger(v)&&v>=0) || condition.min>condition.max)) errors[key]="请输入有效大小区间，下限不能大于上限。";
    if (condition.type === "date_match" && (!Number.isInteger(condition.days)||condition.days<1||condition.days>365000)) errors[key]="天数须为 1 至 365000 的整数。";
    if (rule.target_kind === "directory" && ["extension","size_greater_than","size_less_than","size_between"].includes(condition.type)) errors[key] = "文件夹支持名称和创建、修改时间条件。";
  });
  validateConditions(rule.conditions);
  if (!rule.actions.length || rule.actions.length>20) errors.actions = "请设置 1 至 20 个执行动作。";
  if (rule.target_kind === "directory" && rule.actions.length > 1) errors.actions = "文件夹整理目前每条规则支持一个动作。";
  const variables=[...BUILTIN_VARIABLES,...captureVariables(rule.conditions)];
  rule.actions.forEach((action, index) => {
    const key=`action-${index}`;
    if ((action.type === "move" || action.type === "copy" || action.type === "move_and_extract" || action.type === "organize") && !action.dest.trim()) errors[key] = "请选择目标文件夹。";
    if (action.type === "move_and_extract" && rule.actions.length > 1) errors[`action-${index}`] = "移动并解压已包含整个更新流程，请移除其他动作。";
    if ((action.type === "rename" || action.type === "rename_with_date") && (!action.pattern.trim() || /[\\/<>:"|?*\x00-\x1f]/.test(action.pattern))) errors[key] = "请输入有效重命名格式，不能包含路径或特殊字符。";
    if (action.type === "organize" && action.subfolder && (/^(?:[\\/]|[A-Za-z]:)/.test(action.subfolder) || action.subfolder.split(/[\\/]/).some(p=>!p||p===".."||p==="."||/[<>:"|?*\x00-\x1f]/.test(p)))) errors[key]="子文件夹须为相对路径，不能包含 .. 或特殊字符。";
    const template=action.type === "organize" ? action.subfolder : action.type === "rename" || action.type === "rename_with_date" ? action.pattern : null;
    if(template !== null && /[{}]/.test(template.replace(/\{([^{}]+)\}/g,(match,name)=>variables.includes(name)?"sample":match))) errors[key]="模板变量无效；支持 name、ext、date、datetime、year、month、day 及条件中正则的命名捕获组。";
    if (action.type === "move_to_recycle_bin" && index!==rule.actions.length-1) errors[key]="移入回收站须作为最后一个动作。";
    if (rule.target_kind === "directory" && action.type === "move_and_extract") errors[`action-${index}`] = "移动并解压适用于 ZIP 文件，请选择其他动作。";
  });
  return errors;
}
