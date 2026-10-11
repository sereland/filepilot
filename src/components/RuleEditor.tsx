import { useId, useRef, useState } from "react";
import { api } from "../lib/api";
import { validateRule } from "../lib/ruleValidation";
import { describeAction, describeCondition, directoryConditions, newEmptyRule, type Action, type Condition, type DateSource, type Rule } from "../lib/types";
import ConditionEditor from "./ConditionEditor";
import Modal from "./Modal";
import SelectField from "./SelectField";
import Switch from "./Switch";
import { IconFolder, IconPlus, IconX } from "./icons";

const freshCondition = (type: "extension" | "name_contains"): Condition =>
  type === "extension" ? { type, exts: [] } : { type, keyword: "" };
const freshAction = (type: "move" | "copy" | "rename" | "move_to_recycle_bin" | "move_and_extract"): Action =>
  type === "move" || type === "copy" ? { type, dest: "" } :
  type === "move_and_extract" ? { type, dest: "", retention: "all" } :
  type === "rename" ? { type, pattern: "{name}_{date}.{ext}" } : { type };

export default function RuleEditor({ initial, monitoring, onClose, onSaved }: {
  initial: Rule | null; monitoring: boolean; onClose: () => void; onSaved: (saved: Rule) => Promise<void>;
}) {
  const [rule, setRule] = useState<Rule>(() => {
    const base = initial ?? newEmptyRule();
    return { ...base, target_kind: base.target_kind ?? "file", watch_folders: base.watch_folders.length ? [...base.watch_folders] : [""],
      conditions: base.conditions.length ? [...base.conditions] : [freshCondition("extension")],
      actions: base.actions.length ? [...base.actions] : [freshAction("move")] };
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const fieldsRef = useRef<HTMLFieldSetElement>(null);
  const errorId = useId();
  const errors = submitted ? validateRule(rule) : {};
  const validation = (key: string): FieldValidation => ({ "aria-invalid": errors[key] ? true : undefined, "aria-describedby": errors[key] ? `${errorId}-${key}` : undefined });
  const feedback = (key: string) => errors[key] && <p className="field-error" id={`${errorId}-${key}`}>{errors[key]}</p>;
  const set = (patch: Partial<Rule>) => setRule((current) => ({ ...current, ...patch }));
  const isDirectory = rule.target_kind === "directory";
  const objectName = isDirectory ? "文件夹" : "文件";
  const changeTarget = (target: Rule["target_kind"]) => set({ target_kind: target,
    conditions: target === "directory" ? directoryConditions(rule.conditions) : rule.conditions,
    actions: target === "directory" ? rule.actions.slice(0, 1).map((action) => action.type === "move_and_extract" ? freshAction("move") : (action.type === "rename" || action.type === "rename_with_date") && action.pattern === "{name}_{date}.{ext}" ? { ...action, pattern: "{name}_{date}" } : action) : rule.actions,
  });
  const pick = async (update: (folder: string) => void) => {
    try { const folder = await api.pickFolder(); if (folder) update(folder); }
    catch (e) { setError("无法选择文件夹：" + String(e)); }
  };
  const save = async () => {
    setSubmitted(true);
    setError("");
    if (Object.keys(validateRule(rule)).length) {
      requestAnimationFrame(() => {
        const first = fieldsRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]');
        first?.focus({ preventScroll: true });
        first?.scrollIntoView({ block: "center", behavior: "instant" });
      });
      return;
    }
    const folders = [...new Set(rule.watch_folders.map((folder) => folder.trim()).filter(Boolean))];
    setError(""); setSaving(true);
    try {
      const saved = await api.saveRule({ ...rule, name: rule.name.trim(), watch_folders: folders,
        conditions: rule.conditions.map((condition) => condition.type === "name_contains" ? { ...condition, keyword: condition.keyword.trim() } : condition),
        actions: rule.actions.map((action) => action.type === "move" || action.type === "copy" || action.type === "move_and_extract" || action.type === "organize" ? { ...action, dest: action.dest.trim() } : action) });
      await onSaved(saved);
    } catch (e) { setError("保存失败：" + String(e)); }
    finally { setSaving(false); }
  };
  return <Modal title={initial?.id ? "编辑规则" : "新建规则"} description="选择来源，定义条件，再指定去向。标 * 的项目为必填。" onClose={onClose} busy={saving} closeOnBackdrop={false}
    footer={<><span className="foot-note">保存后可先预览整理效果</span><button className="btn btn-secondary" onClick={onClose} disabled={saving}>取消</button><button className="btn btn-primary" onClick={save} disabled={saving}>{saving ? "保存中…" : "保存规则"}</button></>}>
    {Object.keys(errors).length > 0 && <p className="validation-summary" role="alert">还有 {Object.keys(errors).length} 处需要补充或修改，请检查红框标出的项目。</p>}
    <fieldset ref={fieldsRef} className="editor-fields" disabled={saving}>
      <div className="field-control"><label className="field"><span>规则名称<RequiredMark /></span><input autoFocus aria-required="true" {...validation("name")} value={rule.name} onChange={(e) => set({ name: e.target.value })} placeholder="例如：图片自动归档" maxLength={80} /></label>{feedback("name")}</div>
      <section className="editor-group"><h3>整理对象</h3><SelectField aria-label="整理对象" value={rule.target_kind} onChange={(e) => changeTarget(e.target.value as Rule["target_kind"])}><option value="file">文件</option><option value="directory">文件夹</option></SelectField><p className="editor-help">{isDirectory ? "整理来源第一层的整个文件夹，包含内部文件；同名加编号保留。支持同盘移动、复制（可跨盘）、重命名和回收站。" : "仅整理来源第一层的文件，不扫描子文件夹。"}</p></section>
      <section className="editor-group"><h3>来源文件夹<RequiredMark /></h3>
        {rule.watch_folders.map((folder, index) => <div className="editor-row" key={index}>
          <div className="field-control"><div className="path-field"><input aria-label={"来源文件夹 " + (index + 1)} aria-required="true" {...validation(`source-${index}`)} value={folder} placeholder="选择或粘贴文件夹路径" onChange={(e) => set({ watch_folders: rule.watch_folders.map((value, i) => i === index ? e.target.value : value) })} />
            <button type="button" className="btn btn-secondary" onClick={() => pick((value) => set({ watch_folders: rule.watch_folders.map((old, i) => i === index ? value : old) }))}><IconFolder />选择文件夹</button>
          </div>{feedback(`source-${index}`)}</div>
          {rule.watch_folders.length > 1 && <button className="icon-btn" aria-label="移除来源文件夹" onClick={() => set({ watch_folders: rule.watch_folders.filter((_, i) => i !== index) })}><IconX /></button>}
        </div>)}
        <button className="text-btn add-control" onClick={() => set({ watch_folders: [...rule.watch_folders, ""] })}><IconPlus />添加来源文件夹</button>
      </section>
      <section className="editor-group"><h3>匹配条件<RequiredMark /></h3>
        <ConditionEditor conditions={rule.conditions} isDirectory={isDirectory} submitted={submitted} onChange={conditions=>set({conditions})}/>
        {Object.entries(errors).filter(([key])=>key.startsWith("condition")).map(([key,message])=><p key={key} className="field-error">{message}</p>)}
      </section>
      <section className="editor-group"><h3>执行动作<RequiredMark />{rule.actions.length > 1 && <span>按以下顺序执行</span>}</h3>
        {rule.actions.map((action, index) => <div className="editor-row" key={index}>
          <SelectField aria-label={"动作类型 " + (index + 1)} value={action.type === "organize" ? action.operation : action.type === "rename_with_date" ? "rename" : action.type} onChange={(e) => set({ actions: rule.actions.map((value, i) => i === index ? (isDirectory && e.target.value === "rename" ? { type: "rename", pattern: "{name}_{date}" } : freshAction(e.target.value as Parameters<typeof freshAction>[0])) : value) })}>
            <option value="move">移动到</option><option value="copy">复制到</option><option value="rename">重命名为</option><option value="move_to_recycle_bin">移入回收站</option>
            {!isDirectory && <option value="move_and_extract">移动并解压到</option>}
          </SelectField>
          <div className="condition-value"><ActionFields objectName={objectName} action={action} validation={validation(`action-${index}`)} pick={pick} onChange={(value) => set({ actions: rule.actions.map((old, i) => i === index ? value : old) })} />{feedback(`action-${index}`)}</div>
          {rule.actions.length > 1 && <button className="icon-btn" aria-label="删除动作" onClick={() => set({ actions: rule.actions.filter((_, i) => i !== index) })}><IconX /></button>}
        </div>)}
        {feedback("actions")}
        {!isDirectory && <button className="text-btn add-control" disabled={rule.actions.some((action) => action.type === "move_and_extract")} onClick={() => set({ actions: [...rule.actions, freshAction("move")] })}><IconPlus />添加动作</button>}
        {isDirectory && <p className="editor-help">每条文件夹规则执行一个动作。内部仍在写入时会等待；不自动合并或替换其他版本。</p>}
        {rule.actions.some((action) => (action.type === "rename" || action.type === "rename_with_date")) && <p className="editor-help">支持 {"{name}"} 原{objectName}名、{!isDirectory && <>{"{ext}"} 扩展名、</>}{"{date}"} 日期、{"{datetime}"} 日期时间、{"{year}"} 年、{"{month}"} 月、{"{day}"} 日。移动或重命名后继续处理新路径；复制后仍处理原文件。</p>}
      </section>
      <div className="auto-option"><div><strong>自动整理</strong><p>{rule.enabled ? monitoring ? `来源文件夹出现符合条件的${objectName}时自动整理。` : "已选择自动整理；全局恢复后开始监控。" : `仅在你手动预览并确认后整理${objectName}。`}</p></div><Switch checked={rule.enabled} onChange={() => set({ enabled: !rule.enabled })} label="这条规则的自动整理" /></div>
      <div className="live-summary"><strong>这条规则会这样工作</strong><span>在 {rule.watch_folders.filter(Boolean).join("、") || "来源文件夹"} 中，{rule.conditions.map((condition) => condition.type === "extension" && !condition.exts.length ? "扩展名是所选文件类型" : condition.type === "name_contains" && !condition.keyword ? `${objectName}名包含指定关键词` : describeCondition(condition, rule.target_kind)).join(" 且 ")} 的{objectName}，将{rule.actions.map((action) => (action.type === "move" || action.type === "copy" || action.type === "move_and_extract") && !action.dest ? (action.type === "move" ? "移动到" : action.type === "copy" ? "复制到" : "移动并解压到") + "目标文件夹" : describeAction(action)).join("；")}。{rule.enabled ? "参与自动整理。" : "仅手动执行。"}</span></div>
    </fieldset>
    {error && <p className="error" role="alert">{error}</p>}
  </Modal>;
}

type FieldValidation = { "aria-invalid"?: boolean; "aria-describedby"?: string };
function RequiredMark() { return <b className="required-mark" aria-hidden="true">*</b>; }
function ActionFields({ objectName, action, onChange, pick, validation }: { objectName: string; action: Action; onChange: (action: Action) => void; pick: (update: (folder: string) => void) => Promise<void>; validation: FieldValidation }) {
  if (action.type === "move_and_extract") return <div className="archive-fields">
    <div className="path-field"><input aria-label="目标文件夹" aria-required="true" {...validation} value={action.dest} onChange={(e) => onChange({ ...action, dest: e.target.value })} placeholder="选择存放 ZIP 和解压目录的文件夹" /><button className="btn btn-secondary" onClick={() => pick((dest) => onChange({ ...action, dest }))}><IconFolder />选择文件夹</button></div>
    <label className="archive-retention"><span>保留方式</span><SelectField aria-label="压缩包保留方式" value={action.retention} onChange={(e) => onChange({ ...action, retention: e.target.value as "all" | "latest" })}><option value="all">保留全部</option><option value="latest">只保留最新一份</option></SelectField></label>
    <p className="editor-help">{action.retention === "latest" ? "同组 ZIP 按修改时间保留最新一份，下载编号不作为版本号。新包解压成功后替换旧包和旧目录，保留上一版供整包撤销；下一次成功更新后，更早备份进入回收站。" : "每个 ZIP 单独解压，重名时为 ZIP 和目录一起添加编号。"} 仅支持普通 ZIP。</p>
  </div>;
  if (action.type === "move" || action.type === "copy" || action.type === "organize") {
    const configured = action.type === "organize" ? action : { type:"organize" as const, operation:action.type, dest:action.dest, subfolder:"", date_source:"now" as const };
    return <div className="archive-fields"><div className="path-field"><input aria-label="目标文件夹" aria-required="true" {...validation} value={action.dest} onChange={(e) => onChange({ ...action, dest:e.target.value })} placeholder="选择或粘贴目标路径"/><button className="btn btn-secondary" onClick={()=>pick(dest=>onChange({...action,dest}))}><IconFolder/>选择文件夹</button></div>
      <div className="template-options"><label className="field"><span>子文件夹模板（可选）</span><input aria-label="子文件夹模板" {...validation} value={configured.subfolder} placeholder="例如：{year}/{month} 或 {ext}" onChange={e=>onChange({...configured,subfolder:e.target.value})}/></label><DateSourceField value={configured.date_source} onChange={date_source=>onChange({...configured,date_source})}/></div>
      <p className="editor-help">留空直接放入目标文件夹。支持 {"{year}/{month}/{day}"}、{"{ext}"}、{"{name}"}；预览显示实际路径。</p>
    </div>;
  }
  if (action.type === "rename" || action.type === "rename_with_date") {
    const configured=action.type === "rename_with_date" ? action : {type:"rename_with_date" as const,pattern:action.pattern,date_source:"now" as const};
    return <div className="archive-fields"><input aria-label="重命名格式" aria-required="true" {...validation} value={action.pattern} onChange={e=>onChange({...configured,pattern:e.target.value})} placeholder="{date}_{name}.{ext}"/><DateSourceField value={configured.date_source} onChange={date_source=>onChange({...configured,date_source})}/></div>;
  }
  return <span className="muted small">{objectName}进入系统回收站，可在回收站恢复。</span>;
}

function DateSourceField({value,onChange}:{value:DateSource;onChange:(source:DateSource)=>void}) {
  return <label className="field date-source-field"><span>日期依据</span><SelectField aria-label="日期依据" value={value} onChange={e=>onChange(e.target.value as DateSource)}><option value="now">执行日期</option><option value="created">文件创建日期</option><option value="modified">文件修改日期</option></SelectField></label>;
}
