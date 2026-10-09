import { useState } from "react";
import PreviewDialog from "../components/PreviewDialog";
import RuleEditor from "../components/RuleEditor";
import {
  IconFolder,
  IconInbox,
  IconPlus,
} from "../components/icons";
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
        <div>
          <h2>整理规则</h2>
          <p className="page-sub">设定条件，文件会自动归位。每条规则执行前都可预览，执行后可撤销。</p>
        </div>
        <button className="btn btn-primary" onClick={() => setEditing(null)}>
          <IconPlus />
          新建规则
        </button>
      </div>

      {rules.length === 0 ? (
        <div className="empty">
          <span className="empty-icon">
            <IconInbox />
          </span>
          <p className="empty-title">还没有规则</p>
          <p className="muted">建一条试试，比如</p>
          <span className="empty-example">下载文件夹里扩展名为 pdf 的文件 → 移动到 D:\文档</span>
        </div>
      ) : (
        <div className="rule-grid">
          {rules.map((rule) => (
            <div key={rule.id} className={`rule-card ${rule.enabled ? "" : "disabled"}`}>
              <div className="card-row">
                <span className="rule-name" title={rule.name}>
                  {rule.name}
                </span>
                <label className="switch" title={rule.enabled ? "停用规则" : "启用规则"}>
                  <input
                    type="checkbox"
                    checked={rule.enabled}
                    onChange={() => toggle(rule)}
                  />
                  <span className="slider" />
                </label>
              </div>
              <div className="rule-folder" title={rule.watch_folders.join("\n")}>
                <IconFolder />
                {rule.watch_folders.join("、") || "未设置监控文件夹"}
              </div>
              <div className="rule-logic">
                <div className="logic-line">
                  <span className="logic-k">如果</span>
                  <span>{rule.conditions.map(describeCondition).join(" 且 ") || "—"}</span>
                </div>
                <div className="logic-line">
                  <span className="logic-k">那么</span>
                  <span>{rule.actions.map(describeAction).join("；") || "—"}</span>
                </div>
              </div>
              <div className="card-actions">
                <button className="link-btn" onClick={() => doPreview(rule)}>
                  预览
                </button>
                <button className="link-btn" onClick={() => setEditing(rule)}>
                  编辑
                </button>
                <button className="link-btn danger" onClick={() => doDelete(rule)}>
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
