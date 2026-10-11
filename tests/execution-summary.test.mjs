import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

async function moduleUrl(source, imports = {}) {
  const text = await readFile(new URL(source, import.meta.url), "utf8");
  let { outputText } = ts.transpileModule(text, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  for (const [name, url] of Object.entries(imports)) outputText = outputText.replaceAll(JSON.stringify(name), JSON.stringify(url));
  return "data:text/javascript;base64," + Buffer.from(outputText).toString("base64");
}
const uiUrl = await moduleUrl("../src/lib/ui.ts");
const { formatExecutionTime, formatLastRun } = await import(uiUrl);
const { summarizeExecution, executionStatusLabel } = await import(await moduleUrl("../src/lib/executionSummary.ts", { "./ui": uiUrl }));
const record = (overrides = {}) => ({ batch_id: "latest", timestamp: "2026-10-11T02:42:00Z", rule_name: "安装包归档", src: "C:/Downloads/package.zip", error: null, undone: false, ...overrides });

test("empty history has no recent execution", () => {
  assert.equal(summarizeExecution([]), null);
});
test("full batch deduplicates multiple actions beyond the recent 100 records", () => {
  const records = Array.from({ length: 250 }, (_, i) => record({ src: `C:/Downloads/${i}.zip` }));
  records.push(record({ src: "c:\\downloads\\0.zip" }));
  records.push(record({ batch_id: "older", src: "C:/older.zip" }));
  const summary = summarizeExecution(records);
  assert.equal(summary.fileCount, 250);
  assert.equal(summary.batchId, "latest");
  assert.equal(summary.ruleName, "安装包归档");
  assert.equal(summary.status, "completed");
});
test("successful files and whole folders are counted separately", () => {
  const summary = summarizeExecution([record(), record({ src: "C:/project", is_directory: true }), record({ src: "c:\\PROJECT", is_directory: true })]);
  assert.equal(summary.fileCount, 1);
  assert.equal(summary.directoryCount, 1);
});
test("a failed latest attempt is shown rather than an older successful batch", () => {
  const summary = summarizeExecution([record({ error: "拒绝访问" }), record({ batch_id: "older" })]);
  assert.equal(summary.fileCount, 0);
  assert.equal(summary.failedCount, 1);
  assert.equal(summary.status, "failed");
  assert.equal(executionStatusLabel(summary), "执行失败");
});
test("partial failures do not inflate affected counts or duplicate failed sources", () => {
  const summary = summarizeExecution([record(), record({ src: "C:/locked.zip", error: "锁定" }), record({ src: "c:\\LOCKED.zip", error: "锁定" })]);
  assert.equal(summary.fileCount, 1);
  assert.equal(summary.failedCount, 1);
  assert.equal(executionStatusLabel(summary), "部分失败");
});
test("undo keeps the original execution time and counts while reporting its state", () => {
  const summary = summarizeExecution([record({ undone: true }), record({ src: "C:/second.zip", undone: true })]);
  assert.equal(summary.fileCount, 2);
  assert.equal(summary.timestamp, record().timestamp);
  assert.equal(executionStatusLabel(summary), "已撤销");
  assert.equal(executionStatusLabel(summarizeExecution([record({ undone: true }), record({ src: "C:/second.zip" })])), "部分撤销");
});
test("successful operations with warnings have a visible reminder", () => {
  assert.equal(executionStatusLabel(summarizeExecution([record({ warning: "旧备份待清理" })])), "有提醒");
});
test("relative dates use local calendar days and retain the existing card prefix", () => {
  const now = new Date(2026, 9, 11, 0, 5);
  const today = new Date(2026, 9, 11, 0, 1).toISOString();
  const yesterday = new Date(2026, 9, 10, 23, 59).toISOString();
  assert.equal(formatExecutionTime(today, now), "今天 00:01");
  assert.equal(formatExecutionTime(yesterday, now), "昨天 23:59");
  assert.equal(formatLastRun(today, now), "上次整理：今天 00:01");
  assert.match(formatExecutionTime(new Date(2025, 9, 7, 23, 54).toISOString(), now), /2025/);
  assert.equal(formatExecutionTime("invalid", now), "时间未知");
});
