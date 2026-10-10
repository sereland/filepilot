import { useState } from "react";
import Modal from "../components/Modal";
import PreviewDialog from "../components/PreviewDialog";
import Switch from "../components/Switch";
import { IconEye, IconFolder, IconInbox, IconMore, IconPlus, IconSearch } from "../components/icons";
import { api } from "../lib/api";
import { describeAction, describeCondition, type PlanItem, type Rule } from "../lib/types";
import { uniqueFileCount, type Notify } from "../lib/ui";

interface Props {
  rules: Rule[]; monitoring: boolean; onReload: () => Promise<void>; onEdit: (rule: Rule | null) => void;
  onTemplates: () => void; onLogs: () => void; notify: Notify;
}
export default function RulesView({ rules, monitoring, onReload, onEdit, onTemplates, onLogs, notify }: Props) {
  const [filter, setFilter] = useState<"all" | "auto" | "manual">("all");
  const [query, setQuery] = useState("");
  const [preview, setPreview] = useState<{ rule: Rule; items: PlanItem[] } | null>(null);
  const [deleting, setDeleting] = useState<Rule | null>(null);
  const [pending, setPending] = useState("");
  const [running, setRunning] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const [deleteError, setDeleteError] = useState("");
  const active = rules.filter((rule) => rule.enabled).length;
  const visible = rules.filter((rule) => (filter === "all" || rule.enabled === (filter === "auto")) &&
    [rule.name, ...rule.watch_folders, ...rule.actions.map(describeAction)].join(" ").toLowerCase().includes(query.trim().toLowerCase()));
  const perform = async (id: string, task: () => Promise<void>) => {
    if (pending) return;
    setPending(id);
    try { await task(); } catch (e) { if (deleting) setDeleteError("删除失败：" + String(e)); else notify("操作失败：" + String(e), { error: true }); }
    finally { setPending(""); }
  };
  const doPreview = (rule: Rule) => perform(rule.id, async () => { setPreviewError(""); setPreview({ rule, items: await api.previewRule(rule.id) }); });
  const apply = async () => {
    if (!preview || running) return;
    setRunning(true); setPreviewError("");
    try {
      const records = await api.applyRuleNow(preview.rule.id);
      setPreview(null); await onReload();
      notify(records.length ? "已整理 " + uniqueFileCount(records) + " 个文件。" : "没有文件被处理，文件可能已发生变化。", { action: { label: "查看记录", onClick: onLogs } });
    } catch (e) { setPreviewError("整理失败：" + String(e)); }
    finally { setRunning(false); }
  };
  return <div>
    <header className="page-header"><div><h1>整理规则</h1><p className="page-sub">按条件匹配文件，整理到指定位置。</p></div><button className="btn btn-primary" onClick={() => onEdit(null)}><IconPlus />新建规则</button></header>
    <div className="toolbar"><div className="filters" role="group" aria-label="规则筛选">
      {([{ key: "all", label: "全部", count: rules.length }, { key: "auto", label: "自动整理", count: active }, { key: "manual", label: "仅手动", count: rules.length - active }] as const).map((item) => <button key={item.key} className={"filter" + (filter === item.key ? " active" : "")} aria-pressed={filter === item.key} onClick={() => setFilter(item.key)}>{item.label}<span>{item.count}</span></button>)}
    </div><label className="search"><IconSearch /><input aria-label="搜索规则或文件夹" placeholder="搜索规则或文件夹" value={query} onChange={(e) => setQuery(e.target.value)} /></label></div>
    {!monitoring && active > 0 && <div className="pause-notice">自动整理已暂停。自动规则的配置会保留，仍可手动预览和执行。</div>}
    {visible.length === 0 ? <div className="empty"><IconInbox /><h2>{rules.length ? "没有符合条件的规则" : "从第一条规则开始"}</h2><p>{rules.length ? "试试其他关键词或筛选条件。" : "选择一个常用模板，先预览，再整理。"}</p><button className="btn btn-secondary" onClick={() => { if (!rules.length) onTemplates(); else { setQuery(""); setFilter("all"); } }}>{rules.length ? "清除筛选" : "浏览规则模板"}</button></div> :
      <div className="rule-grid">{visible.map((rule) => <article className="rule-card" key={rule.id}>
        <div className="card-row"><div className="rule-heading"><h2 className="rule-name" title={rule.name}>{rule.name}</h2><span className={"rule-status" + (rule.enabled && monitoring ? " auto" : "")}>{rule.enabled ? monitoring ? "自动整理" : "自动整理 · 已暂停" : "仅手动"}</span></div>
          <div className="rule-mode"><span>自动</span><Switch checked={rule.enabled} disabled={!!pending} label={"自动整理：" + rule.name} onChange={() => { void perform(rule.id, async () => {
            if (!await api.setRuleEnabled(rule.id, !rule.enabled)) throw new Error("规则不存在，请刷新后重试。");
            await onReload(); notify(rule.enabled ? "已改为仅手动整理。" : monitoring ? "已开启这条规则的自动整理。" : "已选择自动整理，全局恢复后运行。");
          }); }} /></div></div>
        <div className="rule-folder"><IconFolder /><span title={rule.watch_folders.join("\n")}>{rule.watch_folders.join("、") || "未设置来源文件夹"}</span></div>
        <dl className="rule-logic"><dt>如果</dt><dd>{rule.conditions.map((condition, index) => <span key={index}>{index > 0 && " 且 "}{condition.type === "extension" ? <>扩展名是 {condition.exts.map((ext) => <span key={ext} className="extension">.{ext.replace(/^\./, "")}</span>)}</> : describeCondition(condition)}</span>)}</dd>
          <dt>那么</dt><dd>{rule.actions.map((action, index) => <span key={index}>{index > 0 && "；"}{action.type === "move" || action.type === "copy" ? <>{action.type === "move" ? "移动到 " : "复制到 "}<span className="path-token" title={action.dest}><IconFolder /><span>{action.dest}</span></span></> : describeAction(action)}</span>)}</dd></dl>
        <div className="card-actions"><button className="btn btn-secondary btn-small" disabled={!!pending} onClick={() => { void doPreview(rule); }}><IconEye />{pending === rule.id ? "处理中…" : "预览整理"}</button><button className="text-btn" onClick={() => onEdit(rule)} disabled={!!pending}>编辑</button>
          <details className="more" onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget)) e.currentTarget.open = false; }} onKeyDown={(e) => { if (e.key === "Escape") { e.currentTarget.open = false; e.currentTarget.querySelector("summary")?.focus(); } }}><summary aria-label={rule.name + "的更多操作"}><IconMore /></summary><div className="menu">
            <button disabled={!!pending} onClick={(e) => { e.currentTarget.closest("details")!.open = false; void perform(rule.id, async () => { await api.saveRule({ ...rule, id: "", name: rule.name + "（副本）", enabled: false }); await onReload(); notify("已复制规则，副本默认仅手动。"); }); }}>复制规则</button>
            <button className="danger" disabled={!!pending} onClick={(e) => { e.currentTarget.closest("details")!.open = false; setDeleteError(""); setDeleting(rule); }}>删除规则</button>
          </div></details>
        </div>
      </article>)}</div>}
    {preview && <PreviewDialog ruleName={preview.rule.name} items={preview.items} running={running} error={previewError} onConfirm={() => { void apply(); }} onClose={() => { if (!running) setPreview(null); }} />}
    {deleting && <Modal compact title="删除这条规则？" onClose={() => setDeleting(null)} busy={!!pending} footer={<><button className="btn btn-secondary" disabled={!!pending} onClick={() => setDeleting(null)}>取消</button><button className="btn btn-danger" disabled={!!pending} onClick={() => { void perform(deleting.id, async () => { if (!await api.deleteRule(deleting.id)) throw new Error("规则不存在。"); setDeleting(null); await onReload(); notify("规则已删除。"); }); }}>{pending ? "删除中…" : "删除规则"}</button></>}><p>将删除「{deleting.name}」。</p><p className="muted">已整理的文件和操作记录会保留。</p>{deleteError && <p className="error" role="alert">{deleteError}</p>}</Modal>}
  </div>;
}
