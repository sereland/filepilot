// 与 src-tauri/core/src/rules.rs 的 serde 定义一一对应

export type Condition =
  | { type: "extension"; exts: string[] }
  | { type: "name_contains"; keyword: string }
  | { type: "size_greater_than"; bytes: number }
  | { type: "created_within_days"; days: number }
  | { type: "name_match"; operator: "contains" | "not_contains" | "equals" | "starts_with" | "ends_with"; keyword: string }
  | { type: "size_less_than"; bytes: number }
  | { type: "size_between"; min: number; max: number }
  | { type: "date_match"; field: "created" | "modified"; operator: "within_days" | "older_than_days"; days: number }
  | { type: "group"; mode: "all" | "any"; negate: boolean; conditions: Condition[] };

export type DateSource = "now" | "created" | "modified";

export type Action =
  | { type: "move"; dest: string }
  | { type: "copy"; dest: string }
  | { type: "rename"; pattern: string }
  | { type: "move_to_recycle_bin" }
  | { type: "move_and_extract"; dest: string; retention: "all" | "latest" }
  | { type: "organize"; operation: "move" | "copy"; dest: string; subfolder: string; date_source: DateSource }
  | { type: "rename_with_date"; pattern: string; date_source: DateSource };
// A package update includes movement, extraction and its undo backup.

export interface Rule {
  id: string;
  name: string;
  enabled: boolean;
  target_kind: "file" | "directory";
  watch_folders: string[];
  conditions: Condition[];
  actions: Action[];
}

export interface PlanItem {
  src: string;
  input_src?: string;
  action_desc: string;
  dest: string | null;
  kind: string; // move | copy | rename | recycle
  rule_name: string;
  is_directory?: boolean;
  source_digest?: string;
  error?: string;
  archive?: {
    root: string; retention: "all" | "latest"; archive_dest: string; folder_dest: string;
    obsolete_sources: string[]; old_targets: string[]; cleanup_backup: string | null; error: string | null;
    expected_state: Record<string, string>;
  };
}

export interface OpRecord {
  id: string;
  batch_id: string;
  timestamp: string;
  rule_id: string;
  rule_name: string;
  kind: string;
  src: string;
  origin_src?: string;
  dest: string | null;
  undone: boolean;
  is_directory?: boolean;
  error?: string; warning?: string; undo_unavailable?: string;
}

export interface RuleLastRun {
  batch_id: string;
  timestamp: string;
  file_count: number;
  directory_count?: number;
}

export const CONDITION_LABELS: Record<Condition["type"], string> = {
  extension: "扩展名",
  name_contains: "文件名包含",
  size_greater_than: "文件大小",
  created_within_days: "创建时间",
  name_match: "名称", size_less_than: "文件大小", size_between: "文件大小区间", date_match: "文件时间", group: "条件组",
};

export const ACTION_LABELS: Record<Action["type"], string> = {
  move: "移动到文件夹",
  copy: "复制到文件夹",
  rename: "重命名",
  move_to_recycle_bin: "移入回收站",
  move_and_extract: "移动并解压到",
  organize: "按模板归档", rename_with_date: "按文件日期重命名",
};

export function describeCondition(c: Condition, target: Rule["target_kind"] = "file"): string {
  const name = target === "directory" ? "文件夹" : "文件";
  switch (c.type) {
    case "extension":
      return `扩展名为 ${c.exts.join("、")}`;
    case "name_contains":
      return `${name}名包含「${c.keyword}」`;
    case "size_greater_than":
      return `文件大于 ${formatBytes(c.bytes)}`;
    case "created_within_days":
      return `${c.days} 天内创建的${name}`;
    case "name_match": return `${name}名${({contains:"包含",not_contains:"不包含",equals:"等于",starts_with:"开头是",ends_with:"结尾是"})[c.operator]}「${c.keyword}」`;
    case "size_less_than": return `文件小于 ${formatBytes(c.bytes)}`;
    case "size_between": return `文件大小 ${formatBytes(c.min)} 至 ${formatBytes(c.max)}`;
    case "date_match": return `${c.field === "created" ? "创建" : "修改"}时间${c.operator === "within_days" ? "在最近" : "距今至少"} ${c.days} 天`;
    case "group": return `${c.negate ? "排除" : ""}（${c.conditions.map(child=>describeCondition(child,target)).join(c.mode === "all" ? " 且 " : " 或 ")}）`;
  }
}

export function describeAction(a: Action): string {
  switch (a.type) {
    case "move":
      return `移动到 ${a.dest}`;
    case "copy":
      return `复制到 ${a.dest}`;
    case "rename":
      return `重命名为 ${a.pattern}`;
    case "move_to_recycle_bin":
      return "移入回收站";
    case "move_and_extract":
      return `移动并解压到 ${a.dest}（${a.retention === "latest" ? "只保留最新一份" : "保留全部"}）`;
    case "organize": return `${a.operation === "move" ? "移动到" : "复制到"} ${a.dest}${a.subfolder ? "/"+a.subfolder : ""}（按${dateSourceLabel(a.date_source)}）`;
    case "rename_with_date": return `重命名为 ${a.pattern}（按${dateSourceLabel(a.date_source)}）`;
  }
}

export function dateSourceLabel(source: DateSource) { return { now:"执行日期", created:"创建日期", modified:"修改日期" }[source]; }

export function directoryConditions(conditions: Condition[]): Condition[] {
  return conditions.map(condition => condition.type === "group" ? { ...condition, conditions:directoryConditions(condition.conditions) } :
    ["extension","size_greater_than","size_less_than","size_between"].includes(condition.type) ? {type:"name_contains",keyword:""} : condition);
}

export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return u === 0 ? `${bytes} B` : `${v.toFixed(1)} ${units[u]}`;
}

export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function newEmptyRule(): Rule {
  return {
    id: "",
    name: "",
    enabled: false,
    target_kind: "file",
    watch_folders: [],
    conditions: [],
    actions: [],
  };
}
