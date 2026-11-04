import { describeAction, describeCondition, type Action, type Condition, type Rule } from "./types";

/** 清理编辑中的规则，供保存和实时预览共用。 */
export function normalizeRule(rule: Rule): Rule {
  return { ...rule, name: rule.name.trim(),
    watch_folders: [...new Set(rule.watch_folders.map((folder) => folder.trim()).filter(Boolean))],
    conditions: rule.conditions.map((condition) => condition.type === "name_contains" ? { ...condition, keyword: condition.keyword.trim() } : condition),
    actions: rule.actions.map((action) => action.type === "move" || action.type === "copy" || action.type === "move_and_extract" || action.type === "organize" ? { ...action, dest: action.dest.trim() } : action) };
}

/** 规则的一句话描述；未填写的部分用占位说明代替，包括条件组内部。 */
export function summarizeRule(rule: Rule): string {
  const objectName = rule.target_kind === "directory" ? "文件夹" : "文件";
  const sources = rule.watch_folders.map((folder) => folder.trim()).filter(Boolean).join("、") || "来源文件夹";
  const conditions = rule.conditions.map((condition) => conditionText(condition, rule.target_kind)).filter(Boolean).join(" 且 ") || `所有${objectName}`;
  return `在 ${sources} 中，${conditions} 的${objectName}，将${rule.actions.map(actionText).join("；")}。${rule.enabled ? "参与自动整理。" : "仅手动执行。"}`;
}

function conditionText(condition: Condition, target: Rule["target_kind"]): string {
  const objectName = target === "directory" ? "文件夹" : "文件";
  switch (condition.type) {
    case "extension": return condition.exts.length ? describeCondition(condition, target) : "扩展名是所选文件类型";
    case "name_contains": return condition.keyword.trim() ? describeCondition(condition, target) : `${objectName}名包含指定关键词`;
    case "name_match": return condition.keyword.trim() ? describeCondition(condition, target) : `${objectName}名符合指定条件`;
    case "group": {
      const parts = condition.conditions.map((child) => conditionText(child, target)).filter(Boolean);
      if (!parts.length) return "";
      const joined = parts.join(condition.mode === "all" ? " 且 " : " 或 ");
      if (condition.negate) return `排除（${joined}）`;
      return parts.length === 1 ? joined : `（${joined}）`;
    }
    default: return describeCondition(condition, target);
  }
}

function actionText(action: Action): string {
  if ((action.type === "move" || action.type === "copy" || action.type === "move_and_extract" || action.type === "organize") && !action.dest.trim()) {
    const verb = action.type === "organize" ? (action.operation === "move" ? "移动到" : "复制到") : { move: "移动到", copy: "复制到", move_and_extract: "移动并解压到" }[action.type];
    return verb + "目标文件夹";
  }
  return describeAction(action);
}
