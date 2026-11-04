import { useId, useRef, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { captureVariables, validateRule } from "../lib/ruleValidation";
import { autoRuleName, normalizeRule } from "../lib/ruleSummary";
import { directoryConditions, newEmptyRule, type Action, type Condition, type DateSource, type Rule } from "../lib/types";
import ConditionEditor from "./ConditionEditor";
import LivePreview from "./LivePreview";
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
  const autoName = autoRuleName(rule);
  const named = { ...rule, name: rule.name.trim() || autoName };
  const errors = submitted ? validateRule(named) : {};
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
    if (Object.keys(validateRule(named)).length) {
      requestAnimationFrame(() => {
        const first = fieldsRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]');
        first?.focus({ preventScroll: true });
        first?.scrollIntoView({ block: "center", behavior: "instant" });
      });
      return;
    }
    const normalized = normalizeRule(named);
    setError(""); setSaving(true);
    try {
      const saved = await api.saveRule(normalized);
      await onSaved(saved);
    } catch (e) { setError("保存失败：" + String(e)); }
    finally { setSaving(false); }
  };
  const variables = captureVariables(rule.conditions);
  return <Modal title={initial?.id ? "编辑规则" : "新建规则"} onClose={onClose} busy={saving} closeOnBackdrop={false}
    titleExtra={<div className="rule-name-field"><input aria-label="规则名称" {...validation("name")} value={rule.name} placeholder={autoName} maxLength={80} disabled={saving} onChange={(e) => set({ name: e.target.value })} />{!rule.name.trim() && <span>自动命名 · 可修改</span>}</div>}
    aside={<LivePreview rule={rule} />}
    footer={<><span className="foot-note">保存后仍需确认预览才会整理已有{objectName}</span><button className="btn btn-secondary" onClick={onClose} disabled={saving}>取消</button><button className="btn btn-primary" onClick={save} disabled={saving}>{saving ? "保存中…" : "保存规则"}</button></>}>
    {Object.keys(errors).length > 0 && <p className="validation-summary" role="alert">还有 {Object.keys(errors).length} 处需要补充或修改，请检查红框标出的项目。</p>}
    <fieldset ref={fieldsRef} className="editor-fields rule-form" disabled={saving}>
      <Step no={1} title="从哪里整理" required>
        <div className={"source-rows" + (rule.watch_folders.length > 1 ? " multi" : "")}>
          {rule.watch_folders.map((folder, index) => <div className="source-row" key={index}>
            {index === 0 ? <div className="segmented" role="group" aria-label="整理对象">{(["file", "directory"] as const).map((kind) => <button type="button" key={kind} className={rule.target_kind === kind ? "active" : ""} aria-pressed={rule.target_kind === kind} onClick={() => rule.target_kind !== kind && changeTarget(kind)}>{kind === "file" ? "文件" : "文件夹"}</button>)}</div> : <span />}
            <div className="field-control"><div className="path-field"><input aria-label={"来源文件夹 " + (index + 1)} aria-required="true" data-autofocus={index === 0 && !initial?.id ? true : undefined} {...validation(`source-${index}`)} value={folder} placeholder="选择或粘贴来源文件夹路径" onChange={(e) => set({ watch_folders: rule.watch_folders.map((value, i) => i === index ? e.target.value : value) })} />
              <button type="button" className="btn btn-secondary" title="选择文件夹" onClick={() => pick((value) => set({ watch_folders: rule.watch_folders.map((old, i) => i === index ? value : old) }))}><IconFolder />选择</button>
            </div>{feedback(`source-${index}`)}</div>
            {rule.watch_folders.length > 1 && <button className="icon-btn" aria-label="移除来源文件夹" onClick={() => set({ watch_folders: rule.watch_folders.filter((_, i) => i !== index) })}><IconX /></button>}
          </div>)}
        </div>
        <div className="step-links"><button className="text-btn add-control" onClick={() => set({ watch_folders: [...rule.watch_folders, ""] })}><IconPlus />添加来源</button>
          <span className="editor-help">{isDirectory ? "整理来源第一层的整个文件夹（含内部文件），同名加编号保留。" : "只整理来源第一层的文件，不扫描子文件夹。"}</span></div>
      </Step>
      <Step no={2} title={`哪些${objectName}`} hint="名称不区分大小写" required>
        <ConditionEditor conditions={rule.conditions} isDirectory={isDirectory} submitted={submitted} onChange={conditions=>set({conditions})}/>
        {Object.entries(errors).filter(([key])=>key.startsWith("condition")).map(([key,message])=><p key={key} className="field-error">{message}</p>)}
      </Step>
      <Step no={3} title="怎么处理" hint={rule.actions.length > 1 ? "按以下顺序执行" : undefined} required>
        {rule.actions.map((action, index) => <div className="editor-row" key={index}>
          <SelectField aria-label={"动作类型 " + (index + 1)} value={action.type === "organize" ? action.operation : action.type === "rename_with_date" ? "rename" : action.type} onChange={(e) => set({ actions: rule.actions.map((value, i) => i === index ? (isDirectory && e.target.value === "rename" ? { type: "rename", pattern: "{name}_{date}" } : freshAction(e.target.value as Parameters<typeof freshAction>[0])) : value) })}>
            <option value="move">移动到</option><option value="copy">复制到</option><option value="rename">重命名为</option><option value="move_to_recycle_bin">移入回收站</option>
            {!isDirectory && <option value="move_and_extract">移动并解压到</option>}
          </SelectField>
          <div className="condition-value"><ActionFields isDirectory={isDirectory} variables={variables} action={action} validation={validation(`action-${index}`)} pick={pick} onChange={(value) => set({ actions: rule.actions.map((old, i) => i === index ? value : old) })} />{feedback(`action-${index}`)}</div>
          {rule.actions.length > 1 && <button className="icon-btn" aria-label="删除动作" onClick={() => set({ actions: rule.actions.filter((_, i) => i !== index) })}><IconX /></button>}
        </div>)}
        {feedback("actions")}
        {!isDirectory && <div className="step-links"><button className="text-btn add-control" disabled={rule.actions.some((action) => action.type === "move_and_extract")} onClick={() => set({ actions: [...rule.actions, freshAction("move")] })}><IconPlus />添加动作</button></div>}
        {isDirectory && <p className="editor-help">每条文件夹规则执行一个动作。内部仍在写入时会等待；不自动合并或替换其他版本。</p>}
        {rule.actions.length > 1 && <p className="editor-help">移动或重命名后继续处理新路径；复制后仍处理原文件。</p>}
      </Step>
      <Step no={4} title="自动整理">
        <div className="auto-option"><p>{rule.enabled ? monitoring ? `来源文件夹出现符合条件的${objectName}时自动整理。` : "已选择自动整理；全局恢复后开始监控。" : `仅在你手动预览并确认后整理${objectName}。`}</p><Switch checked={rule.enabled} onChange={() => set({ enabled: !rule.enabled })} label="这条规则的自动整理" /></div>
      </Step>
    </fieldset>
    {error && <p className="error" role="alert">{error}</p>}
  </Modal>;
}

function Step({ no, title, hint, required, children }: { no: number; title: string; hint?: string; required?: boolean; children: ReactNode }) {
  return <section className="editor-step"><span className="step-no" aria-hidden="true">{no}</span><div className="step-body"><h3>{title}{required && <RequiredMark />}{hint && <span>{hint}</span>}</h3>{children}</div></section>;
}

type FieldValidation = { "aria-invalid"?: boolean; "aria-describedby"?: string };
function RequiredMark() { return <b className="required-mark" aria-hidden="true">*</b>; }
const DATE_VARIABLE = /\{(date|datetime|year|month|day)\}/;
type Chip = { token: string; label: string; capture?: boolean };
const FOLDER_CHIPS: Chip[] = [{ token: "{year}", label: "年份" }, { token: "{month}", label: "月份" }, { token: "{day}", label: "日" }, { token: "{ext}", label: "扩展名" }, { token: "{name}", label: "原名" }];
const RENAME_CHIPS: Chip[] = [{ token: "{name}", label: "原名" }, { token: "{ext}", label: "扩展名" }, { token: "{date}", label: "日期" }, { token: "{year}", label: "年份" }, { token: "{month}", label: "月份" }];

function ActionFields({ isDirectory, variables, action, onChange, pick, validation }: { isDirectory: boolean; variables: string[]; action: Action; onChange: (action: Action) => void; pick: (update: (folder: string) => void) => Promise<void>; validation: FieldValidation }) {
  const templateRef = useRef<HTMLInputElement>(null);
  const objectName = isDirectory ? "文件夹" : "文件";
  const chips = (base: Chip[]) => [...variables.map((name) => ({ token: `{${name}}`, label: name, capture: true })), ...base.filter((chip) => !isDirectory || chip.token !== "{ext}")];
  const insert = (current: string, token: string, apply: (value: string) => void, separator: string) => {
    const input = templateRef.current;
    const focused = !!input && document.activeElement === input;
    const start = focused ? input.selectionStart ?? current.length : current.length, end = focused ? input.selectionEnd ?? current.length : current.length;
    const before = current.slice(0, start);
    // Keep variables apart: {year} + {month} becomes {year}/{month}, not {year}{month}.
    const text = before && !/[\\/_\-. ]$/.test(before) && (!focused || before.endsWith("}")) ? separator + token : token;
    apply(current.slice(0, start) + text + current.slice(end));
    requestAnimationFrame(() => { input?.focus(); input?.setSelectionRange(start + text.length, start + text.length); });
  };
  if (action.type === "move_and_extract") return <div className="archive-fields">
    <div className="path-field"><input aria-label="目标文件夹" aria-required="true" {...validation} value={action.dest} onChange={(e) => onChange({ ...action, dest: e.target.value })} placeholder="选择存放 ZIP 和解压目录的文件夹" /><button className="btn btn-secondary" title="选择文件夹" onClick={() => pick((dest) => onChange({ ...action, dest }))}><IconFolder />选择</button></div>
    <label className="archive-retention"><span>保留方式</span><SelectField aria-label="压缩包保留方式" value={action.retention} onChange={(e) => onChange({ ...action, retention: e.target.value as "all" | "latest" })}><option value="all">保留全部</option><option value="latest">只保留最新一份</option></SelectField></label>
    <p className="editor-help">{action.retention === "latest" ? "同组 ZIP 按修改时间保留最新一份，下载编号不作为版本号。新包解压成功后替换旧包和旧目录，保留上一版供整包撤销；下一次成功更新后，更早备份进入回收站。" : "每个 ZIP 单独解压，重名时为 ZIP 和目录一起添加编号。"} 仅支持普通 ZIP。</p>
  </div>;
  if (action.type === "move" || action.type === "copy" || action.type === "organize") {
    const configured = action.type === "organize" ? action : { type:"organize" as const, operation:action.type, dest:action.dest, subfolder:"", date_source:"now" as const };
    const setSubfolder = (subfolder: string) => onChange({ ...configured, subfolder });
    return <div className="archive-fields"><div className="path-field"><input aria-label="目标文件夹" aria-required="true" {...validation} value={action.dest} onChange={(e) => onChange({ ...action, dest:e.target.value })} placeholder="选择或粘贴目标文件夹路径"/><button className="btn btn-secondary" title="选择文件夹" onClick={()=>pick(dest=>onChange({...action,dest}))}><IconFolder/>选择</button></div>
      <div className="template-row"><span className="template-label">子文件夹</span><input ref={templateRef} className="mono" aria-label="子文件夹模板" {...validation} value={configured.subfolder} spellCheck={false} placeholder="可选，留空直接放入目标文件夹" onChange={e=>setSubfolder(e.target.value)}/></div>
      {DATE_VARIABLE.test(configured.subfolder) && <DateSourceField value={configured.date_source} onChange={date_source=>onChange({...configured,date_source})}/>}
      <VariableChips chips={chips(FOLDER_CHIPS)} onInsert={(token) => insert(configured.subfolder, token, setSubfolder, "/")} />
    </div>;
  }
  if (action.type === "rename" || action.type === "rename_with_date") {
    const configured=action.type === "rename_with_date" ? action : {type:"rename_with_date" as const,pattern:action.pattern,date_source:"now" as const};
    const setPattern = (pattern: string) => onChange({ ...configured, pattern });
    return <div className="archive-fields"><div className="template-row"><input ref={templateRef} className="mono" aria-label="重命名格式" aria-required="true" {...validation} value={action.pattern} spellCheck={false} onChange={e=>setPattern(e.target.value)} placeholder={isDirectory ? "{date}_{name}" : "{date}_{name}.{ext}"}/></div>
      {DATE_VARIABLE.test(action.pattern) && <DateSourceField value={configured.date_source} onChange={date_source=>onChange({...configured,date_source})}/>}
      <VariableChips chips={chips(RENAME_CHIPS)} onInsert={(token) => insert(action.pattern, token, setPattern, "_")} />
    </div>;
  }
  return <span className="muted small action-note">{objectName}进入系统回收站，可在回收站恢复。</span>;
}

function VariableChips({ chips, onInsert }: { chips: Chip[]; onInsert: (token: string) => void }) {
  const captures = chips.filter((chip) => chip.capture).length;
  return <div className="variable-chips" role="group" aria-label="插入变量"><span>点击插入</span><div className="chip-list">
    {chips.map((chip, index) => <button type="button" key={chip.token} className={"variable-chip" + (chip.capture ? ` capture c${index % 3}` : "") + (index === captures && captures ? " after-captures" : "")} title={chip.capture ? `正则提取：${chip.token}` : chip.token}
      onMouseDown={(e) => e.preventDefault()} onClick={() => onInsert(chip.token)}>{chip.label}</button>)}
  </div></div>;
}

function DateSourceField({value,onChange}:{value:DateSource;onChange:(source:DateSource)=>void}) {
  return <div className="template-row"><span className="template-label">日期依据</span><SelectField className="date-source" aria-label="日期依据" title="日期依据" value={value} onChange={e=>onChange(e.target.value as DateSource)}><option value="now">按执行日期</option><option value="created">按创建日期</option><option value="modified">按修改日期</option></SelectField></div>;
}
