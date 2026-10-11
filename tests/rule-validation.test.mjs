import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

async function load(source) {
  const text = await readFile(new URL(source, import.meta.url), "utf8");
  const { outputText } = ts.transpileModule(text, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import("data:text/javascript;base64," + Buffer.from(outputText).toString("base64"));
}
const { validateRule } = await load("../src/lib/ruleValidation.ts");
const { newEmptyRule, describeCondition } = await load("../src/lib/types.ts");
const { itemCountLabel, countLabel } = await load("../src/lib/ui.ts");
const directoryRule = () => ({ ...newEmptyRule(), name: "项目目录", target_kind: "directory", watch_folders: ["C:/Downloads"],
  conditions: [{ type: "name_contains", keyword: "project" }], actions: [{ type: "move", dest: "C:/Projects" }] });

test("valid directory rule uses folder names and a single ordinary action", () => {
  assert.deepEqual(validateRule(directoryRule()), {});
  assert.equal(describeCondition(directoryRule().conditions[0], "directory"), "文件夹名包含「project」");
});
test("directory rules reject file-only conditions, extraction and multiple actions", () => {
  const rule = directoryRule();
  rule.conditions = [{ type: "extension", exts: ["zip"] }, { type: "size_greater_than", bytes: 1024 }];
  rule.actions = [{ type: "move_and_extract", dest: "C:/Packages", retention: "latest" }, { type: "move_to_recycle_bin" }];
  const errors = validateRule(rule);
  for (const key of ["condition-0", "condition-1", "action-0", "actions"]) assert.ok(errors[key], key);
});
test("file rules still support extension, size and ZIP updates", () => {
  const rule = { ...directoryRule(), target_kind: "file", conditions: [{ type: "extension", exts: ["zip"] }, { type: "size_greater_than", bytes: 1024 }],
    actions: [{ type: "move_and_extract", dest: "C:/Packages", retention: "latest" }] };
  assert.deepEqual(validateRule(rule), {});
  assert.equal(newEmptyRule().target_kind, "file");
});
test("folder validation still identifies missing names, source and destination", () => {
  const rule = { ...directoryRule(), name: " ", watch_folders: [""], conditions: [{ type: "name_contains", keyword: "" }], actions: [{ type: "copy", dest: "" }] };
  const errors = validateRule(rule);
  for (const key of ["name", "source-0", "condition-0", "action-0"]) assert.ok(errors[key], key);
});
test("counts distinguish folders from files and deduplicate multiple actions", () => {
  const records = [{ src: "C:/project", is_directory: true }, { src: "c:\\project", is_directory: true }, { src: "C:/report.pdf" }];
  assert.equal(itemCountLabel(records), "1 个文件、1 个文件夹");
  assert.equal(countLabel(2, 2), "2 个文件夹");
  assert.equal(countLabel(3), "3 个文件");
});
