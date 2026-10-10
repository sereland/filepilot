import { Fragment, useMemo, useState } from "react";
import Modal from "./Modal";
import { IconInbox, IconShield } from "./icons";
import { fileNameOf, type PlanItem } from "../lib/types";
import { sourceKey } from "../lib/ui";

const KIND: Record<string, string> = { move: "移动", copy: "复制", rename: "重命名", recycle: "回收站" };
export default function PreviewDialog({ ruleName, items, running, error, onConfirm, onClose }: {
  ruleName: string; items: PlanItem[]; running: boolean; error?: string; onConfirm: (selectedSources: string[]) => void; onClose: () => void;
}) {
  const files = useMemo(() => {
    const groups = new Map<string, { key: string; src: string; operations: PlanItem[] }>();
    for (const item of items) {
      const key = sourceKey(item.src);
      const group = groups.get(key);
      if (group) group.operations.push(item);
      else groups.set(key, { key, src: item.src, operations: [item] });
    }
    return [...groups.values()];
  }, [items]);
  const [selected, setSelected] = useState(() => new Set(files.map((file) => file.key)));
  const selectedFiles = files.filter((file) => selected.has(file.key));
  const count = selectedFiles.length;
  const operations = selectedFiles.flatMap((file) => file.operations);
  const hasRecycle = operations.some((item) => item.kind === "recycle");
  return <Modal title={"整理预览 · " + ruleName} description="确认文件去向，再执行整理。" onClose={onClose} busy={running}
    footer={<><span className="foot-note">尚未执行，仅整理勾选的文件</span><button className="btn btn-secondary" onClick={onClose} disabled={running}>取消</button><button className="btn btn-primary" onClick={() => onConfirm(selectedFiles.map((file) => file.src))} disabled={!count || running}>{running ? "整理中…" : "确认整理 " + count + " 个文件"}</button></>}>
    {!items.length ? <div className="empty"><IconInbox /><h2>没有文件符合这条规则</h2><p>检查来源文件夹和匹配条件后再试。</p></div> : <>
      <div className="plan-summary"><strong>{count}</strong><span>/ {files.length} 个文件已选{operations.length !== count && " · " + operations.length + " 项操作"}</span></div>
      <p className="selection-help">取消勾选可跳过文件。同一文件的多个动作一起选择；仅影响本次手动整理。</p>
      <div className="table-scroll"><table className="file-table preview-table"><colgroup><col className="selection-col" /><col className="file-col" /><col className="action-col" /><col /></colgroup><thead><tr><th>
        <input type="checkbox" aria-label="全选文件" checked={count === files.length} aria-checked={count > 0 && count < files.length ? "mixed" : count === files.length} ref={(node) => { if (node) node.indeterminate = count > 0 && count < files.length; }} disabled={running} onChange={() => setSelected(count === files.length ? new Set() : new Set(files.map((file) => file.key)))} />
      </th><th>文件</th><th>操作</th><th>目标位置</th></tr></thead><tbody>
        {files.map((file) => <Fragment key={file.key}>{file.operations.map((item, index) => <tr key={index} className={selected.has(file.key) ? "" : "excluded"}>
          {index === 0 && <><td rowSpan={file.operations.length}><input type="checkbox" aria-label={"选择文件：" + file.src} checked={selected.has(file.key)} disabled={running} onChange={() => setSelected((current) => {
            const next = new Set(current); if (next.has(file.key)) next.delete(file.key); else next.add(file.key); return next;
          })} /></td><td rowSpan={file.operations.length} title={file.src}>{fileNameOf(file.src)}</td></>}
          <td>{KIND[item.kind] ?? item.kind}</td><td title={item.dest ?? item.action_desc}>{item.dest ?? item.action_desc}</td>
        </tr>)}</Fragment>)}
      </tbody></table></div>
      <details className="path-details"><summary>查看完整来源与目标路径</summary>{items.map((item, index) => <p key={index}>{item.src}<br />→ {item.dest ?? item.action_desc}</p>)}</details>
      <p className="hint"><IconShield /><span>{hasRecycle ? "回收站中的文件可在系统回收站恢复。" : "操作会记录在日志中，可撤销最近一批整理。"} 自动规则仍按原条件运行。</span></p>
    </>}
    {error && <p className="error" role="alert">{error}</p>}
  </Modal>;
}
