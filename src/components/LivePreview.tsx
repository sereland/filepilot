import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { captureVariables } from "../lib/ruleValidation";
import { normalizeRule, summarizeRule } from "../lib/ruleSummary";
import type { DraftItem, DraftPreview, DraftStep, Rule } from "../lib/types";

const DEBOUNCE_MS = 350;
const UNMATCHED_SHOWN = 6;
const STEP_LABELS: Record<DraftStep["kind"], string> = { move: "移动到", copy: "复制到", rename: "改名为", recycle: "移入回收站", move_extract: "移动并解压到" };

type Result = { key: string; data?: DraftPreview; error?: string };

export default function LivePreview({ rule }: { rule: Rule }) {
  const objectName = rule.target_kind === "directory" ? "文件夹" : "文件";
  const desktop = isTauri();
  // 名称与自动整理开关不影响匹配结果，变化时无需重新扫描。
  const draft = useMemo(() => ({ ...normalizeRule(rule), id: "", name: "", enabled: false }), [rule]);
  const key = useMemo(() => JSON.stringify(draft), [draft]);
  const hasSource = draft.watch_folders.length > 0;
  const [result, setResult] = useState<Result | null>(null);
  useEffect(() => {
    if (!desktop || !hasSource) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api.previewDraft(draft).then((data) => { if (!cancelled) setResult({ key, data }); },
        (error) => { if (!cancelled) setResult({ key, error: String(error) }); });
    }, DEBOUNCE_MS);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [key, desktop, hasSource]); // eslint-disable-line react-hooks/exhaustive-deps
  const loading = desktop && hasSource && result?.key !== key;
  const data = result?.data;
  const problems = data?.items.filter((item) => item.error) ?? [];
  const ready = data?.items.filter((item) => !item.error) ?? [];
  const captureNames = [...new Set([...captureVariables(rule.conditions), ...(data?.items.flatMap((item) => Object.keys(item.captures)) ?? [])])];

  let body: ReactNode;
  if (!desktop) body = <p className="lp-notice">实时预览需要在 FilePilot 桌面应用中查看。</p>;
  else if (!hasSource) body = <p className="lp-notice">选择来源文件夹后，这里会列出符合条件的{objectName}和它们的去向。</p>;
  else if (result?.error) body = <p className="lp-notice warn">暂时无法预览：{result.error}</p>;
  else if (data) body = <>
    {data.missing_folders.length > 0 && <p className="lp-notice warn">找不到来源文件夹：{data.missing_folders.join("、")}</p>}
    {data.condition_error && <p className="lp-notice warn">匹配条件有误：{data.condition_error}</p>}
    {!data.condition_error && data.action_error && data.matched > 0 && <p className="lp-notice">补全执行动作后显示去向：{data.action_error}</p>}
    {!data.condition_error && data.total === 0 && data.missing_folders.length < draft.watch_folders.length && <p className="lp-notice">来源文件夹中暂无可整理的{objectName}（跳过临时文件和正在写入的文件）。</p>}
    {!data.condition_error && data.total > 0 && data.matched === 0 && <p className="lp-notice">当前没有{objectName}符合条件。</p>}
    {ready.length > 0 && <><p className="lp-label">将处理 {ready.length} 个</p>{ready.map((item) => <PreviewItem key={item.src} item={item} captureNames={captureNames} />)}</>}
    {problems.length > 0 && <><p className="lp-label">需要处理 {problems.length} 个（整理时会跳过）</p>{problems.map((item) => <PreviewItem key={item.src} item={item} captureNames={captureNames} />)}</>}
    {data.matched > data.items.length && <p className="lp-label">… 另有 {data.matched - data.items.length} 个符合条件，保存后可在完整预览中查看</p>}
    {data.unmatched.length > 0 && <><p className="lp-label">未命中 {data.total - data.matched} 个（不处理）</p>
      {data.unmatched.slice(0, UNMATCHED_SHOWN).map((name) => <div className="lp-item miss" key={name}><div className="lp-src">{name}</div></div>)}
      {data.total - data.matched > UNMATCHED_SHOWN && <p className="lp-label">… 另有 {data.total - data.matched - UNMATCHED_SHOWN} 个</p>}</>}
  </>;

  return <aside className="live-preview" aria-label="实时预览" aria-busy={loading || undefined}>
    <div className="lp-head">
      <h3><span className={"lp-dot" + (loading ? " loading" : "")} aria-hidden="true" />实时预览{loading && <span className="lp-status">更新中…</span>}</h3>
      <p className="lp-summary">{summarizeRule(rule)}</p>
      {data && !data.condition_error && <div className="lp-stats" aria-live="polite">
        <span className="lp-stat">共 {data.total} 个{objectName}</span><span className="lp-stat ok">命中 {data.matched}</span>
        {problems.length > 0 && <span className="lp-stat warn">需注意 {problems.length}</span>}
      </div>}
    </div>
    <div className="lp-list">{body}</div>
    <p className="lp-foot">基于来源文件夹当前内容模拟，不会移动任何{objectName}。</p>
  </aside>;
}

function PreviewItem({ item, captureNames }: { item: DraftItem; captureNames: string[] }) {
  return <div className={"lp-item" + (item.error ? " warn" : "")}>
    <div className="lp-src" title={item.src}>{highlight(item.name, item.captures, captureNames)}</div>
    {item.steps.map((step, index) => <div className="lp-dest" key={index}><span>{STEP_LABELS[step.kind]}</span>{step.dest && <PathText path={step.dest} nameOnly={step.kind === "rename"} />}</div>)}
    {item.error && <div className="lp-dest">⚠ {item.error}</div>}
  </div>;
}

function PathText({ path, nameOnly }: { path: string; nameOnly: boolean }) {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/")) + 1;
  return <span title={path}>{!nameOnly && path.slice(0, cut)}<b>{path.slice(cut)}</b></span>;
}

/** 在原名中标出各提取变量的取值，颜色按变量名区分。 */
function highlight(name: string, captures: Record<string, string>, captureNames: string[]): ReactNode {
  const ranges: { start: number; end: number; label: string }[] = [];
  for (const [label, value] of Object.entries(captures)) {
    if (!value) continue;
    let from = 0;
    for (;;) {
      const start = name.indexOf(value, from);
      if (start < 0) break;
      const end = start + value.length;
      if (!ranges.some((r) => start < r.end && end > r.start)) { ranges.push({ start, end, label }); break; }
      from = start + 1;
    }
  }
  if (!ranges.length) return name;
  ranges.sort((a, b) => a.start - b.start);
  const parts: ReactNode[] = [];
  let at = 0;
  for (const range of ranges) {
    parts.push(name.slice(at, range.start));
    parts.push(<mark key={range.start} className={`lp-cap c${captureNames.indexOf(range.label) % 3}`} title={`{${range.label}}`}>{name.slice(range.start, range.end)}</mark>);
    at = range.end;
  }
  parts.push(name.slice(at));
  return parts;
}
