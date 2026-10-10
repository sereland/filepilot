import Modal from "./Modal";
import { IconInbox, IconShield } from "./icons";
import { fileNameOf, type PlanItem } from "../lib/types";
import { uniqueFileCount } from "../lib/ui";

const KIND: Record<string, string> = { move: "移动", copy: "复制", rename: "重命名", recycle: "回收站" };
export default function PreviewDialog({ ruleName, items, running, error, onConfirm, onClose }: {
  ruleName: string; items: PlanItem[]; running: boolean; error?: string; onConfirm: () => void; onClose: () => void;
}) {
  const count = uniqueFileCount(items);
  const hasRecycle = items.some((item) => item.kind === "recycle");
  return <Modal title={"整理预览 · " + ruleName} description="确认文件去向，再执行整理。" onClose={onClose} busy={running}
    footer={<><span className="foot-note">尚未执行，本次确认后才整理</span><button className="btn btn-secondary" onClick={onClose} disabled={running}>取消</button><button className="btn btn-primary" onClick={onConfirm} disabled={!items.length || running}>{running ? "整理中…" : "确认整理 " + count + " 个文件"}</button></>}>
    {!items.length ? <div className="empty"><IconInbox /><h2>没有文件符合这条规则</h2><p>检查来源文件夹和匹配条件后再试。</p></div> : <>
      <div className="plan-summary"><strong>{count}</strong><span>个文件符合规则{items.length !== count && " · " + items.length + " 项操作"}</span></div>
      <div className="table-scroll"><table className="file-table"><thead><tr><th>文件</th><th>操作</th><th>目标位置</th></tr></thead><tbody>
        {items.map((item, index) => <tr key={index}><td title={item.src}>{fileNameOf(item.src)}</td><td>{KIND[item.kind] ?? item.kind}</td><td title={item.dest ?? item.action_desc}>{item.dest ?? item.action_desc}</td></tr>)}
      </tbody></table></div>
      <details className="path-details"><summary>查看完整来源与目标路径</summary>{items.map((item, index) => <p key={index}>{item.src}<br />→ {item.dest ?? item.action_desc}</p>)}</details>
      <p className="hint"><IconShield /><span>{hasRecycle ? "回收站中的文件可在系统回收站恢复。" : "操作会记录在日志中，可撤销最近一批整理。"}</span></p>
    </>}
    {error && <p className="error" role="alert">{error}</p>}
  </Modal>;
}
