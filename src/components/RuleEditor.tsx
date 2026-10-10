import { useState } from "react";
import { api } from "../lib/api";
import { describeAction, describeCondition, newEmptyRule, type Action, type Condition, type Rule } from "../lib/types";
import ExtensionPicker from "./ExtensionPicker";
import Modal from "./Modal";
import Switch from "./Switch";
import { IconFolder, IconPlus, IconX } from "./icons";

const freshCondition = (type: Condition["type"]): Condition =>
  type === "extension" ? { type, exts: [] } : type === "name_contains" ? { type, keyword: "" } :
  type === "size_greater_than" ? { type, bytes: 100 * 1024 * 1024 } : { type, days: 7 };
const freshAction = (type: Action["type"]): Action =>
  type === "move" || type === "copy" ? { type, dest: "" } :
  type === "rename" ? { type, pattern: "{name}_{date}.{ext}" } : { type };

export default function RuleEditor({ initial, monitoring, onClose, onSaved }: {
  initial: Rule | null; monitoring: boolean; onClose: () => void; onSaved: (saved: Rule) => Promise<void>;
}) {
  const [rule, setRule] = useState<Rule>(() => {
    const base = initial ?? newEmptyRule();
    return { ...base, watch_folders: base.watch_folders.length ? [...base.watch_folders] : [""],
      conditions: base.conditions.length ? [...base.conditions] : [freshCondition("extension")],
      actions: base.actions.length ? [...base.actions] : [freshAction("move")] };
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const set = (patch: Partial<Rule>) => setRule((current) => ({ ...current, ...patch }));
  const pick = async (update: (folder: string) => void) => {
    try { const folder = await api.pickFolder(); if (folder) update(folder); }
    catch (e) { setError("无法选择文件夹：" + String(e)); }
  };
  const save = async () => {
    const folders = [...new Set(rule.watch_folders.map((folder) => folder.trim()).filter(Boolean))];
    if (!rule.name.trim()) return setError("请给规则起个名字。");
    if (!folders.length) return setError("请至少选择一个来源文件夹。");
    if (!rule.conditions.length || !rule.actions.length) return setError("请至少添加一个匹配条件和一个执行动作。");
    for (const condition of rule.conditions) {
      if (condition.type === "extension" && !condition.exts.length) return setError("请至少选择一种扩展名。");
      if (condition.type === "name_contains" && !condition.keyword.trim()) return setError("请输入文件名关键词。");
      if (condition.type === "size_greater_than" && (!Number.isFinite(condition.bytes) || condition.bytes <= 0)) return setError("文件大小必须大于 0。");
      if (condition.type === "created_within_days" && (!Number.isInteger(condition.days) || condition.days < 1)) return setError("创建天数必须为正整数。");
    }
    for (const action of rule.actions) {
      if ((action.type === "move" || action.type === "copy") && !action.dest.trim()) return setError("请选择目标文件夹。");
      if (action.type === "rename" && !action.pattern.trim()) return setError("请输入重命名格式。");
    }
    setError(""); setSaving(true);
    try {
      const saved = await api.saveRule({ ...rule, name: rule.name.trim(), watch_folders: folders,
        conditions: rule.conditions.map((condition) => condition.type === "name_contains" ? { ...condition, keyword: condition.keyword.trim() } : condition),
        actions: rule.actions.map((action) => action.type === "move" || action.type === "copy" ? { ...action, dest: action.dest.trim() } : action) });
      await onSaved(saved);
    } catch (e) { setError("保存失败：" + String(e)); }
    finally { setSaving(false); }
  };
  return <Modal title={initial?.id ? "编辑规则" : "新建规则"} description="选择文件夹，定义条件，再指定文件去向。" onClose={onClose} busy={saving}
    footer={<><span className="foot-note">保存后可先预览整理效果</span><button className="btn btn-secondary" onClick={onClose} disabled={saving}>取消</button><button className="btn btn-primary" onClick={save} disabled={saving}>{saving ? "保存中…" : "保存规则"}</button></>}>
    <fieldset className="editor-fields" disabled={saving}>
      <label className="field"><span>规则名称</span><input autoFocus value={rule.name} onChange={(e) => set({ name: e.target.value })} placeholder="例如：图片自动归档" maxLength={80} /></label>
      <section className="editor-group"><h3>来源文件夹</h3>
        {rule.watch_folders.map((folder, index) => <div className="editor-row" key={index}>
          <div className="path-field"><input aria-label={"来源文件夹 " + (index + 1)} value={folder} placeholder="选择或粘贴文件夹路径" onChange={(e) => set({ watch_folders: rule.watch_folders.map((value, i) => i === index ? e.target.value : value) })} />
            <button type="button" className="btn btn-secondary" onClick={() => pick((value) => set({ watch_folders: rule.watch_folders.map((old, i) => i === index ? value : old) }))}><IconFolder />选择文件夹</button>
          </div>
          {rule.watch_folders.length > 1 && <button className="icon-btn" aria-label="移除来源文件夹" onClick={() => set({ watch_folders: rule.watch_folders.filter((_, i) => i !== index) })}><IconX /></button>}
        </div>)}
        <button className="text-btn add-control" onClick={() => set({ watch_folders: [...rule.watch_folders, ""] })}><IconPlus />添加来源文件夹</button>
      </section>
      <section className="editor-group"><h3>匹配条件{rule.conditions.length > 1 && <span>以下条件全部满足</span>}</h3>
        {rule.conditions.map((condition, index) => <div className="editor-row" key={index}>
          <select aria-label={"条件类型 " + (index + 1)} value={condition.type} onChange={(e) => set({ conditions: rule.conditions.map((value, i) => i === index ? freshCondition(e.target.value as Condition["type"]) : value) })}>
            <option value="extension">扩展名是</option><option value="name_contains">文件名包含</option><option value="size_greater_than">文件大小大于</option><option value="created_within_days">创建于最近</option>
          </select>
          <div className="condition-value"><ConditionFields condition={condition} onChange={(value) => set({ conditions: rule.conditions.map((old, i) => i === index ? value : old) })} /></div>
          {rule.conditions.length > 1 && <button className="icon-btn" aria-label="删除条件" onClick={() => set({ conditions: rule.conditions.filter((_, i) => i !== index) })}><IconX /></button>}
        </div>)}
        {rule.conditions.some((condition) => condition.type === "extension") && <p className="editor-help">同一扩展名条件中，选中的任一种扩展名都匹配。</p>}
        <button className="text-btn add-control" onClick={() => set({ conditions: [...rule.conditions, freshCondition("extension")] })}><IconPlus />添加条件</button>
      </section>
      <section className="editor-group"><h3>执行动作{rule.actions.length > 1 && <span>按以下顺序执行</span>}</h3>
        {rule.actions.map((action, index) => <div className="editor-row" key={index}>
          <select aria-label={"动作类型 " + (index + 1)} value={action.type} onChange={(e) => set({ actions: rule.actions.map((value, i) => i === index ? freshAction(e.target.value as Action["type"]) : value) })}>
            <option value="move">移动到</option><option value="copy">复制到</option><option value="rename">重命名为</option><option value="move_to_recycle_bin">移入回收站</option>
          </select>
          <div className="condition-value"><ActionFields action={action} pick={pick} onChange={(value) => set({ actions: rule.actions.map((old, i) => i === index ? value : old) })} /></div>
          {rule.actions.length > 1 && <button className="icon-btn" aria-label="删除动作" onClick={() => set({ actions: rule.actions.filter((_, i) => i !== index) })}><IconX /></button>}
        </div>)}
        <button className="text-btn add-control" onClick={() => set({ actions: [...rule.actions, freshAction("move")] })}><IconPlus />添加动作</button>
        {rule.actions.some((action) => action.type === "rename") && <p className="editor-help">支持 {"{name}"} 原文件名、{"{ext}"} 扩展名、{"{date}"} 日期、{"{datetime}"} 日期时间。</p>}
      </section>
      <div className="auto-option"><div><strong>自动整理</strong><p>{rule.enabled ? monitoring ? "来源文件夹出现符合条件的文件时自动整理。" : "已选择自动整理；全局恢复后开始监控。" : "仅在你手动预览并确认后整理文件。"}</p></div><Switch checked={rule.enabled} onChange={() => set({ enabled: !rule.enabled })} label="这条规则的自动整理" /></div>
      <div className="live-summary"><strong>这条规则会这样工作</strong><span>在 {rule.watch_folders.filter(Boolean).join("、") || "来源文件夹"} 中，{rule.conditions.map((condition) => condition.type === "extension" && !condition.exts.length ? "扩展名是所选文件类型" : condition.type === "name_contains" && !condition.keyword ? "文件名包含指定关键词" : describeCondition(condition)).join(" 且 ")} 的文件，将{rule.actions.map((action) => (action.type === "move" || action.type === "copy") && !action.dest ? (action.type === "move" ? "移动到" : "复制到") + "目标文件夹" : describeAction(action)).join("；")}。{rule.enabled ? "参与自动整理。" : "仅手动执行。"}</span></div>
    </fieldset>
    {error && <p className="error" role="alert">{error}</p>}
  </Modal>;
}

function ConditionFields({ condition, onChange }: { condition: Condition; onChange: (condition: Condition) => void }) {
  switch (condition.type) {
    case "extension": return <ExtensionPicker values={condition.exts} onChange={(exts) => onChange({ ...condition, exts })} />;
    case "name_contains": return <input aria-label="文件名关键词" value={condition.keyword} onChange={(e) => onChange({ ...condition, keyword: e.target.value })} placeholder="例如：发票" />;
    case "size_greater_than": return <div className="inline-fields"><input aria-label="文件大小（MB）" type="number" min={1} value={condition.bytes / 1024 / 1024} onChange={(e) => onChange({ ...condition, bytes: Math.round(Number(e.target.value) * 1024 * 1024) })} /><span>MB</span></div>;
    case "created_within_days": return <div className="inline-fields"><input aria-label="最近创建天数" type="number" min={1} step={1} value={condition.days} onChange={(e) => onChange({ ...condition, days: Number(e.target.value) })} /><span>天</span></div>;
  }
}
function ActionFields({ action, onChange, pick }: { action: Action; onChange: (action: Action) => void; pick: (update: (folder: string) => void) => Promise<void> }) {
  if (action.type === "move" || action.type === "copy") return <div className="path-field"><input aria-label="目标文件夹" value={action.dest} onChange={(e) => onChange({ ...action, dest: e.target.value })} placeholder="选择或粘贴目标路径" /><button className="btn btn-secondary" onClick={() => pick((dest) => onChange({ ...action, dest }))}><IconFolder />选择文件夹</button></div>;
  if (action.type === "rename") return <input aria-label="重命名格式" value={action.pattern} onChange={(e) => onChange({ ...action, pattern: e.target.value })} placeholder="{name}_{date}.{ext}" />;
  return <span className="muted small">文件进入系统回收站，可在回收站恢复。</span>;
}
