import type { OpRecord } from "./types";
import { sourceKey } from "./ui";

export interface ExecutionSummary {
  batchId: string;
  timestamp: string;
  ruleName: string;
  fileCount: number;
  directoryCount: number;
  failedCount: number;
  warningCount: number;
  status: "completed" | "failed" | "partial_failure" | "undone" | "partial_undo";
}

// Input comes from getOplogBatch (newest first), including the complete batch.
export function summarizeExecution(records: OpRecord[]): ExecutionSummary | null {
  const latest = records[0];
  if (!latest) return null;
  const batch = records.filter((record) => record.batch_id === latest.batch_id);
  const completed = batch.filter((record) => !record.error);
  const sources = new Map(completed.map((record) => [sourceKey(record.src), !!record.is_directory]));
  const directoryCount = [...sources.values()].filter(Boolean).length;
  const failedCount = new Set(batch.filter((record) => record.error).map((record) => sourceKey(record.src))).size;
  const undoneCount = completed.filter((record) => record.undone).length;
  const status = !completed.length ? "failed" : undoneCount === completed.length ? "undone" :
    undoneCount ? "partial_undo" : failedCount ? "partial_failure" : "completed";
  return {
    batchId: latest.batch_id, timestamp: latest.timestamp, ruleName: latest.rule_name,
    fileCount: sources.size - directoryCount, directoryCount, failedCount,
    warningCount: batch.filter((record) => record.warning).length, status,
  };
}

export function executionStatusLabel(summary: ExecutionSummary): string {
  const labels = { completed: "已完成", failed: "执行失败", partial_failure: "部分失败", undone: "已撤销", partial_undo: "部分撤销" };
  return summary.status === "completed" && summary.warningCount ? "有提醒" : labels[summary.status];
}
