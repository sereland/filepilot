import { useState } from "react";
import { IconInbox, IconUndo } from "../components/icons";
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

interface BatchGroup {
  batch_id: string;
  rule_name: string;
  timestamp: string;
  records: OpRecord[];
}

/** 按批次分组（输入为时间倒序，组内同样倒序） */
function groupByBatch(records: OpRecord[]): BatchGroup[] {
  const groups: BatchGroup[] = [];
  const map = new Map<string, BatchGroup>();
  for (const r of records) {
    let g = map.get(r.batch_id);
    if (!g) {
      g = { batch_id: r.batch_id, rule_name: r.rule_name, timestamp: r.timestamp, records: [] };
      map.set(r.batch_id, g);
      groups.push(g);
    }
    g.records.push(r);
  }
  return groups;
}

function timeAgo(iso: string): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const min = Math.floor((Date.now() - t) / 60000);
  if (min < 1) return "刚刚";
  if (min < 60) return `${min} 分钟前`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} 小时前`;
  const d = Math.floor(h / 24);
  if (d === 1) return "昨天";
  if (d < 30) return `${d} 天前`;
  const dt = new Date(t);
  return `${dt.getMonth() + 1}月${dt.getDate()}日`;
}

function fullTime(iso: string): string {
  const t = new Date(iso);
  if (Number.isNaN(t.getTime())) return iso;
  return t.toLocaleString("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export default function OplogView({ records, onReload }: Props) {
  const [undoing, setUndoing] = useState(false);
  const groups = groupByBatch(records);

  // 下一次撤销将回滚的批次：最新的、还有未撤销记录的组
  const nextUndo = groups.find((g) => g.records.some((r) => !r.undone));
  const nextUndoCount = nextUndo ? nextUndo.records.filter((r) => !r.undone).length : 0;

  const doUndo = async () => {
    if (!nextUndo) return;
    if (!confirm(`确定撤销「${nextUndo.rule_name}」（${nextUndoCount} 个文件）吗？文件将被移回原位置。`)) return;
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

  return (
    <div>
      <div className="page-header">
        <div>
          <h2>操作记录</h2>
          <p className="page-sub">每次整理按批次记录，点撤销可整体回滚最近的一批。</p>
        </div>
        <button className="btn btn-secondary" onClick={doUndo} disabled={!nextUndo || undoing}>
          <IconUndo />
          {undoing ? "撤销中…" : nextUndo ? `撤销：${nextUndo.rule_name}（${nextUndoCount} 个文件）` : "撤销上次整理"}
        </button>
      </div>

      {groups.length === 0 ? (
        <div className="empty">
          <span className="empty-icon">
            <IconInbox />
          </span>
          <p className="empty-title">还没有整理记录</p>
          <p className="muted">每次执行规则，这里都会记下"从哪 → 到哪"，随时可撤销。</p>
        </div>
      ) : (
        <div className="oplog-timeline">
          {groups.map((g) => {
            const undoneCount = g.records.filter((r) => r.undone).length;
            const allUndone = undoneCount === g.records.length;
            const partial = undoneCount > 0 && !allUndone;
            return (
              <div key={g.batch_id} className={`oplog-group ${allUndone ? "undone" : ""}`}>
                <span className="oplog-dot" />
                <div className="oplog-group-header" title={fullTime(g.timestamp)}>
                  <span className="oplog-group-title">{g.rule_name}</span>
                  <span className="muted small">
                    {timeAgo(g.timestamp)} · {g.records.length} 个文件
                  </span>
                  {allUndone && <span className="muted small">已撤销</span>}
                  {partial && <span className="muted small">部分撤销</span>}
                </div>
                <div className="oplog-group-items">
                  {g.records.map((r) => (
                    <div key={r.id} className={`oplog-item ${r.undone ? "undone" : ""}`}>
                      <span className="muted small">{KIND_LABEL[r.kind] ?? r.kind}</span>
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
                      {r.undone && <span className="muted small">已撤销</span>}
                    </div>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
