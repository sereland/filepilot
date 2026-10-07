// 与 src-tauri/core/src/rules.rs 的 serde 定义一一对应

export type Condition =
  | { type: "extension"; exts: string[] }
  | { type: "name_contains"; keyword: string }
  | { type: "size_greater_than"; bytes: number }
  | { type: "created_within_days"; days: number };

export type Action =
  | { type: "move"; dest: string }
  | { type: "copy"; dest: string }
  | { type: "rename"; pattern: string }
  | { type: "move_to_recycle_bin" };

export interface Rule {
  id: string;
  name: string;
  enabled: boolean;
  watch_folders: string[];
  conditions: Condition[];
  actions: Action[];
}

export interface PlanItem {
  src: string;
  action_desc: string;
  dest: string | null;
  kind: string; // move | copy | rename | recycle
  rule_name: string;
}

export interface OpRecord {
  id: string;
  batch_id: string;
  timestamp: string;
  rule_id: string;
  rule_name: string;
  kind: string;
  src: string;
  dest: string | null;
  undone: boolean;
}

export const CONDITION_LABELS: Record<Condition["type"], string> = {
  extension: "扩展名",
  name_contains: "文件名包含",
  size_greater_than: "文件大小",
  created_within_days: "创建时间",
};

export const ACTION_LABELS: Record<Action["type"], string> = {
  move: "移动到文件夹",
  copy: "复制到文件夹",
  rename: "重命名",
  move_to_recycle_bin: "移入回收站",
};

export function describeCondition(c: Condition): string {
  switch (c.type) {
    case "extension":
      return `扩展名为 ${c.exts.join("、")}`;
    case "name_contains":
      return `文件名包含「${c.keyword}」`;
    case "size_greater_than":
      return `文件大于 ${formatBytes(c.bytes)}`;
    case "created_within_days":
      return `${c.days} 天内创建的文件`;
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
  }
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
    enabled: true,
    watch_folders: [],
    conditions: [],
    actions: [],
  };
}
