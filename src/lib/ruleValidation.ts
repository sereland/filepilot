import type { Rule } from "./types";

export function validateRule(rule: Rule): Record<string, string> {
  const errors: Record<string, string> = {};
  if (!rule.name.trim()) errors.name = "请给规则起个名字。";
  if (!rule.watch_folders.length) errors["source-0"] = "请选择来源文件夹。";
  rule.watch_folders.forEach((folder, index) => {
    if (!folder.trim()) errors[`source-${index}`] = rule.watch_folders.length > 1 ? "请选择来源文件夹，或移除这一行。" : "请选择来源文件夹。";
  });
  if (!rule.conditions.length) errors.conditions = "请至少添加一个匹配条件。";
  rule.conditions.forEach((condition, index) => {
    const key = `condition-${index}`;
    if (condition.type === "extension" && !condition.exts.some((ext) => ext.trim().replace(/^\.+/, ""))) errors[key] = "请至少选择一种扩展名。";
    if (condition.type === "name_contains" && !condition.keyword.trim()) errors[key] = "请输入文件名关键词。";
    if (condition.type === "size_greater_than" && (!Number.isFinite(condition.bytes) || condition.bytes <= 0)) errors[key] = "文件大小必须大于 0。";
    if (condition.type === "created_within_days" && (!Number.isInteger(condition.days) || condition.days < 1)) errors[key] = "创建天数必须为正整数。";
  });
  if (!rule.actions.length) errors.actions = "请至少添加一个执行动作。";
  rule.actions.forEach((action, index) => {
    if ((action.type === "move" || action.type === "copy") && !action.dest.trim()) errors[`action-${index}`] = "请选择目标文件夹。";
    if (action.type === "rename" && !action.pattern.trim()) errors[`action-${index}`] = "请输入重命名格式。";
  });
  return errors;
}
