import { useState } from "react";
import { api } from "../lib/api";
import { fileNameOf, type OpRecord } from "../lib/types";

interface Props {
  records: OpRecord[];
  onReload: () => void;
}

const KIND_LABEL: Record<string, string> = {
  move: "移动",
  copy: "复制",
  rename: "重命名",
  recycle: "移入回收站",
};

export default function OplogView({ records, onReload }: Props) {
  const [undoing, setUndoing] = useState(false);

  const doUndo = async () => {
    if (!confirm("确定撤销上次整理吗？文件将被移回原位置。")) return;
    setUndoing(true);
    try {
      const n = await api.undoLast();
      alert(n > 0 ? `已撤销 ${n} 个文件` : "没有可撤销的操作");
      onReload();
    } catch (e) {
      alert(`撤销失败：${e}`);
    } finally {
      setUndoing(false);
    }
  };

  const canUndo = records.some((r) => !r.undone);

  return (
    <div>
      <div className="page-header">
        <h2>操作记录</h2>
        <button className="btn btn-secondary" onClick={doUndo} disabled={!canUndo || undoing}>
          ↩ {undoing ? "撤销中…" : "撤销上次整理"}
        </button>
      </div>

      {records.length === 0 ? (
        <div className="empty">
          <p>还没有整理记录。</p>
          <p className="muted small">每次执行规则，这里都会记下"从哪 → 到哪"，随时可撤销。</p>
        </div>
      ) : (
        <div className="oplog-list">
          {records.map((r) => (
            <div key={r.id} className={`oplog-item ${r.undone ? "undone" : ""}`}>
              <span className="tag">{KIND_LABEL[r.kind] ?? r.kind}</span>
              <span className="plan-src" title={r.src}>
                {fileNameOf(r.src)}
              </span>
              {r.dest && (
                <>
                  <span className="plan-arrow">→</span>
                  <span className="muted small" title={r.dest}>
                    {fileNameOf(r.dest)}
                  </span>
                </>
              )}
              <span className="muted small">{r.rule_name}</span>
              {r.undone && <span className="tag tag-undone">已撤销</span>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
