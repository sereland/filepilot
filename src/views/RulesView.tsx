import { useState } from "react";
import PreviewDialog from "../components/PreviewDialog";
import RuleEditor from "../components/RuleEditor";
import { api } from "../lib/api";
import { describeAction, describeCondition, type PlanItem, type Rule } from "../lib/types";

interface Props {
  rules: Rule[];
  onReload: () => void;
}

export default function RulesView({ rules, onReload }: Props) {
  const [editing, setEditing] = useState<Rule | null | undefined>(undefined);
  const [preview, setPreview] = useState<{ rule: Rule; items: PlanItem[] } | null>(null);
  const [running, setRunning] = useState(false);

  const doPreview = async (rule: Rule) => {
    try {
      const items = await api.previewRule(rule.id);
      setPreview({ rule, items });
    } catch (e) {
      alert(`预览失败：${e}`);
    }
  };

  const doApply = async () => {
    if (!preview) return;
    setRunning(true);
    try {
      const records = await api.applyRuleNow(preview.rule.id);
      setPreview(null);
      onReload();
      if (records.length === 0) alert("没有文件被处理");
    } catch (e) {
      alert(`执行失败：${e}`);
    } finally {
      setRunning(false);
    }
  };

  const doDelete = async (rule: Rule) => {
    if (!confirm(`确定删除规则「${rule.name}」吗？`)) return;
    await api.deleteRule(rule.id);
    onReload();
  };

  const toggle = async (rule: Rule) => {
    await api.setRuleEnabled(rule.id, !rule.enabled);
    onReload();
  };

  return (
    <div>
      <div className="page-header">
        <h2>整理规则</h2>
        <button className="btn btn-primary" onClick={() => setEditing(null)}>
          + 新建规则
        </button>
      </div>

      {rules.length === 0 ? (
        <div className="empty">
          <p>还没有规则。建一条试试，比如：</p>
          <p className="muted">「下载文件夹里扩展名为 pdf 的文件 → 移动到 D:\文档」</p>
        </div>
      ) : (
        <div className="rule-grid">
          {rules.map((rule) => (
            <div key={rule.id} className={`rule-card ${rule.enabled ? "" : "disabled"}`}>
              <div className="card-row">
                <strong>{rule.name}</strong>
                <label className="switch">
                  <input
                    type="checkbox"
                    checked={rule.enabled}
                    onChange={() => toggle(rule)}
                  />
                  <span className="slider" />
                </label>
              </div>
              <div className="muted small">📁 {rule.watch_folders.join("、") || "未设置"}</div>
              <div className="rule-logic">
                <div>
                  <span className="tag tag-if">如果</span>{" "}
                  {rule.conditions.map(describeCondition).join(" 且 ") || "—"}
                </div>
                <div>
                  <span className="tag tag-then">那么</span>{" "}
                  {rule.actions.map(describeAction).join("；") || "—"}
                </div>
              </div>
              <div className="card-actions">
                <button className="btn btn-ghost" onClick={() => doPreview(rule)}>
                  预览
                </button>
                <button className="btn btn-ghost" onClick={() => setEditing(rule)}>
                  编辑
                </button>
                <button className="btn btn-ghost danger" onClick={() => doDelete(rule)}>
                  删除
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {editing !== undefined && (
        <RuleEditor
          initial={editing}
          onClose={() => setEditing(undefined)}
          onSaved={() => {
            setEditing(undefined);
            onReload();
          }}
        />
      )}

      {preview && (
        <PreviewDialog
          ruleName={preview.rule.name}
          items={preview.items}
          running={running}
          onConfirm={doApply}
          onClose={() => setPreview(null)}
        />
      )}
    </div>
  );
}
