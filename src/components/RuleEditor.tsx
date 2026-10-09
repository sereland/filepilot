import { useState } from "react";
import { api } from "../lib/api";
import { IconFolder, IconShield, IconX } from "../components/icons";
import {
  ACTION_LABELS,
  CONDITION_LABELS,
  describeAction,
  describeCondition,
  newEmptyRule,
  type Action,
  type Condition,
  type Rule,
} from "../lib/types";

interface Props {
  initial: Rule | null; // null = 新建
  onClose: () => void;
  onSaved: () => void;
}

export default function RuleEditor({ initial, onClose, onSaved }: Props) {
  const [rule, setRule] = useState<Rule>(initial ?? newEmptyRule());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const set = (patch: Partial<Rule>) => setRule((r) => ({ ...r, ...patch }));

  const addFolder = async () => {
    try {
      const folder = await api.pickFolder();
      if (folder && !rule.watch_folders.includes(folder)) {
        set({ watch_folders: [...rule.watch_folders, folder] });
      }
    } catch {
      setError("无法打开文件夹选择器");
    }
  };

  const addCondition = (type: Condition["type"]) => {
    const c: Condition =
      type === "extension"
        ? { type, exts: [] }
        : type === "name_contains"
          ? { type, keyword: "" }
          : type === "size_greater_than"
            ? { type, bytes: 100 * 1024 * 1024 }
            : { type, days: 7 };
    set({ conditions: [...rule.conditions, c] });
  };

  const addAction = (type: Action["type"]) => {
    const a: Action =
      type === "move"
        ? { type, dest: "" }
        : type === "copy"
          ? { type, dest: "" }
          : type === "rename"
            ? { type, pattern: "{name}_{date}.{ext}" }
            : { type };
    set({ actions: [...rule.actions, a] });
  };

  const save = async () => {
    if (!rule.name.trim()) return setError("请给规则起个名字");
    if (rule.watch_folders.length === 0) return setError("请至少选择一个监控文件夹");
    if (rule.conditions.length === 0) return setError("请至少添加一个条件");
    if (rule.actions.length === 0) return setError("请至少添加一个动作");
    setSaving(true);
    try {
      await api.saveRule(rule);
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>{initial ? "编辑规则" : "新建规则"}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            <IconX />
          </button>
        </div>

        <div className="modal-body">
          <label className="field">
            <span>规则名称</span>
            <input
              value={rule.name}
              onChange={(e) => set({ name: e.target.value })}
              placeholder="例如：PDF 自动归档"
            />
          </label>

          <div className="section">
            <div className="section-title">监控这些文件夹</div>
            {rule.watch_folders.map((f) => (
              <div key={f} className="chip-row">
                <span className="chip" title={f}>
                  <IconFolder />
                  {f}
                </span>
                <button
                  className="icon-btn"
                  aria-label="移除文件夹"
                  onClick={() =>
                    set({ watch_folders: rule.watch_folders.filter((x) => x !== f) })
                  }
                >
                  <IconX />
                </button>
              </div>
            ))}
            <button className="btn btn-secondary" onClick={addFolder}>
              + 选择文件夹
            </button>
          </div>

          <div className="section">
            <div className="section-title">
              如果 <span className="muted">（以下条件全部满足）</span>
            </div>
            {rule.conditions.map((c, i) => (
              <div key={i} className="card">
                <div className="card-row">
                  <strong>{CONDITION_LABELS[c.type]}</strong>
                  <button
                    className="icon-btn"
                    aria-label="删除条件"
                    onClick={() =>
                      set({ conditions: rule.conditions.filter((_, j) => j !== i) })
                    }
                  >
                    <IconX />
                  </button>
                </div>
                <ConditionFields
                  condition={c}
                  onChange={(nc) =>
                    set({
                      conditions: rule.conditions.map((x, j) => (j === i ? nc : x)),
                    })
                  }
                />
                <div className="muted small">{describeCondition(c)}</div>
              </div>
            ))}
            <div className="add-row">
              {(Object.keys(CONDITION_LABELS) as Condition["type"][]).map((t) => (
                <button key={t} className="btn btn-ghost" onClick={() => addCondition(t)}>
                  + {CONDITION_LABELS[t]}
                </button>
              ))}
            </div>
          </div>

          <div className="section">
            <div className="section-title">那么执行</div>
            {rule.actions.map((a, i) => (
              <div key={i} className="card">
                <div className="card-row">
                  <strong>{ACTION_LABELS[a.type]}</strong>
                  <button
                    className="icon-btn"
                    aria-label="删除动作"
                    onClick={() =>
                      set({ actions: rule.actions.filter((_, j) => j !== i) })
                    }
                  >
                    <IconX />
                  </button>
                </div>
                <ActionFields
                  action={a}
                  onChange={(na) =>
                    set({ actions: rule.actions.map((x, j) => (j === i ? na : x)) })
                  }
                />
                <div className="muted small">{describeAction(a)}</div>
              </div>
            ))}
            <div className="add-row">
              {(Object.keys(ACTION_LABELS) as Action["type"][]).map((t) => (
                <button key={t} className="btn btn-ghost" onClick={() => addAction(t)}>
                  + {ACTION_LABELS[t]}
                </button>
              ))}
            </div>
            {rule.actions.some((a) => a.type === "rename") && (
              <div className="hint">
                <IconShield />
                <span>
                  重命名支持变量：{"{name}"} 原文件名、{"{ext}"} 扩展名、{"{date}"}{" "}
                  日期(2026-10-07)、{"{datetime}"} 日期时间
                </span>
              </div>
            )}
          </div>

          {error && <div className="error">{error}</div>}
        </div>

        <div className="modal-footer">
          <button className="btn btn-secondary" onClick={onClose}>
            取消
          </button>
          <button className="btn btn-primary" onClick={save} disabled={saving}>
            {saving ? "保存中…" : "保存规则"}
          </button>
        </div>
      </div>
    </div>
  );
}

function ConditionFields({
  condition,
  onChange,
}: {
  condition: Condition;
  onChange: (c: Condition) => void;
}) {
  switch (condition.type) {
    case "extension":
      return (
        <input
          value={condition.exts.join(", ")}
          onChange={(e) =>
            onChange({
              ...condition,
              exts: e.target.value.split(/[,，\s]+/).filter(Boolean),
            })
          }
          placeholder="pdf, docx, xlsx（逗号分隔）"
        />
      );
    case "name_contains":
      return (
        <input
          value={condition.keyword}
          onChange={(e) => onChange({ ...condition, keyword: e.target.value })}
          placeholder="例如：发票"
        />
      );
    case "size_greater_than":
      return (
        <div className="inline-fields">
          <input
            type="number"
            min={1}
            value={Math.round(condition.bytes / 1024 / 1024)}
            onChange={(e) =>
              onChange({
                ...condition,
                bytes: Math.max(1, Number(e.target.value) || 1) * 1024 * 1024,
              })
            }
          />
          <span>MB</span>
        </div>
      );
    case "created_within_days":
      return (
        <div className="inline-fields">
          <input
            type="number"
            min={1}
            value={condition.days}
            onChange={(e) =>
              onChange({ ...condition, days: Math.max(1, Number(e.target.value) || 1) })
            }
          />
          <span>天内创建</span>
        </div>
      );
  }
}

function ActionFields({
  action,
  onChange,
}: {
  action: Action;
  onChange: (a: Action) => void;
}) {
  switch (action.type) {
    case "move":
    case "copy":
      return (
        <div className="inline-fields">
          <input
            value={action.dest}
            onChange={(e) => onChange({ ...action, dest: e.target.value })}
            placeholder="目标文件夹路径，例如 D:\文档"
            style={{ flex: 1 }}
          />
          <button
            className="btn btn-secondary"
            onClick={async () => {
              const f = await api.pickFolder();
              if (f) onChange({ ...action, dest: f });
            }}
          >
            选择
          </button>
        </div>
      );
    case "rename":
      return (
        <input
          value={action.pattern}
          onChange={(e) => onChange({ ...action, pattern: e.target.value })}
          placeholder="{name}_{date}.{ext}"
        />
      );
    case "move_to_recycle_bin":
      return <div className="muted small">文件将被移入系统回收站，可手动恢复。</div>;
  }
}
