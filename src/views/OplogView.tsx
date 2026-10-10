import { useState } from "react";
import Modal from "../components/Modal";
import { IconInbox, IconUndo } from "../components/icons";
import { api } from "../lib/api";
import { fileNameOf, type OpRecord } from "../lib/types";
import { uniqueFileCount, type Notify } from "../lib/ui";

const KIND: Record<string, string> = { move: "移动", copy: "复制", rename: "重命名", recycle: "移入回收站" };
function groupByBatch(records: OpRecord[]) {
  const groups: { id: string; name: string; timestamp: string; records: OpRecord[] }[] = [];
  const map = new Map<string, typeof groups[number]>();
  for (const record of records) {
    let group = map.get(record.batch_id);
    if (!group) { group = { id: record.batch_id, name: record.rule_name, timestamp: record.timestamp, records: [] }; map.set(group.id, group); groups.push(group); }
    group.records.push(record);
  }
  return groups;
}
function formatTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
export default function OplogView({ records, onReload, notify }: { records: OpRecord[]; onReload: () => Promise<void>; notify: Notify }) {
  const [confirming, setConfirming] = useState(false);
  const [undoing, setUndoing] = useState(false);
  const [undoError, setUndoError] = useState("");
  const groups = groupByBatch(records);
  const next = groups.find((group) => group.records.some((record) => !record.undone));
  const undo = async () => {
    if (!next || undoing) return;
    setUndoing(true); setUndoError("");
    try { const count = await api.undoLast(); setConfirming(false); await onReload(); notify(count > 0 ? "已撤销 " + count + " 项操作。" : "没有操作被撤销，文件可能已发生变化。"); }
    catch (e) { setUndoError("撤销失败：" + String(e)); }
    finally { setUndoing(false); }
  };
  return <div>
    <header className="page-header"><div><h1>操作记录</h1><p className="page-sub">每一批整理都有记录，文件去向一目了然。</p></div><button className="btn btn-secondary" onClick={() => { setUndoError(""); setConfirming(true); }} disabled={!next || undoing}><IconUndo />{undoing ? "撤销中…" : "撤销上次整理"}</button></header>
    {!groups.length ? <div className="empty"><IconInbox /><h2>还没有整理记录</h2><p>执行规则后，可以在这里查看文件去向和撤销结果。</p></div> :
      <><p className="section-label">最近的整理{records.length >= 100 && " · 显示最近 100 项操作"}</p><div className="oplog-list">{groups.map((group) => {
        const undone = group.records.filter((record) => record.undone).length;
        const count = uniqueFileCount(group.records);
        return <details className="oplog-group" key={group.id}><summary><span className="oplog-group-title">{group.name}</span><span className="muted small">{formatTime(group.timestamp)} · {count} 个文件{count !== group.records.length && " · " + group.records.length + " 项操作"}{undone ? undone === group.records.length ? " · 已撤销" : " · 部分撤销" : ""}</span></summary>
          <div className="table-scroll"><table className="file-table"><thead><tr><th>操作</th><th>来源文件</th><th>目标位置</th><th>状态</th></tr></thead><tbody>{group.records.map((record) => <tr key={record.id}><td>{KIND[record.kind] ?? record.kind}</td><td><details className="record-path"><summary title={record.src}>{fileNameOf(record.src)}</summary><span>{record.src}</span></details></td><td>{record.dest ?? "系统回收站"}</td><td className="muted">{record.undone ? "已撤销" : "已完成"}</td></tr>)}</tbody></table></div>
        </details>;
      })}</div></>}
    {confirming && next && <Modal compact title="撤销上次整理？" onClose={() => setConfirming(false)} busy={undoing} footer={<><button className="btn btn-secondary" disabled={undoing} onClick={() => setConfirming(false)}>取消</button><button className="btn btn-primary" disabled={undoing} onClick={() => { void undo(); }}>{undoing ? "撤销中…" : "确认撤销"}</button></>}><p>将回滚最近一批「{next.name}」中尚未撤销的操作。</p><p className="muted small">移动和重命名会恢复原位置，复制会删除副本。撤销结果以文件的当前状态为准。</p>{undoError && <p className="error" role="alert">{undoError}</p>}</Modal>}
  </div>;
}
