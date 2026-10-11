import { Fragment, useMemo, useState } from "react";
import Modal from "./Modal";
import { IconInbox, IconShield } from "./icons";
import { fileNameOf, type PlanItem } from "../lib/types";
import { sourceKey } from "../lib/ui";

const KIND: Record<string, string> = { move: "移动", copy: "复制", rename: "重命名", recycle: "回收站", move_extract: "移动并解压" };
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
  const eligible = files.filter((file) => !file.operations.some((item) => item.archive?.error));
  const [selected, setSelected] = useState(() => new Set(eligible.map((file) => file.key)));
  const selectedFiles = files.filter((file) => selected.has(file.key));
  const count = selectedFiles.length;
  const operations = selectedFiles.flatMap((file) => file.operations);
  const hasRecycle = operations.some((item) => item.kind === "recycle");
  const hasArchive = items.some((item) => item.archive);
  const unit = hasArchive ? "项整理" : "个文件";
  return <Modal title={"整理预览 · " + ruleName} description="确认文件去向，再执行整理。" onClose={onClose} busy={running}
    footer={<><span className="foot-note">尚未执行，仅整理勾选的项目</span><button className="btn btn-secondary" onClick={onClose} disabled={running}>取消</button><button className="btn btn-primary" onClick={() => onConfirm([...new Set(selectedFiles.flatMap((file) => [file.src, ...file.operations.flatMap((item) => item.archive?.obsolete_sources ?? [])]))])} disabled={!count || running}>{running ? "整理中…" : "确认整理 " + count + (hasArchive ? " 项" : " 个文件")}</button></>}>
    {!items.length ? <div className="empty"><IconInbox /><h2>没有文件符合这条规则</h2><p>检查来源文件夹和匹配条件后再试。</p></div> : <>
      <div className="plan-summary"><strong>{count}</strong><span>/ {files.length} {unit}已选{operations.length !== count && " · " + operations.length + " 项操作"}</span></div>
      <p className="selection-help">{hasArchive ? "同组 ZIP 作为一项整理，旧包和目录随整包一起撤销。取消勾选可跳过整组。" : "取消勾选可跳过文件。同一文件的多个动作一起选择；仅影响本次手动整理。"}</p>
      {items.filter((item) => item.archive).map((item) => <div className="archive-preview" key={item.src}>
        <strong>{fileNameOf(item.src)} · {item.archive!.retention === "latest" ? "只保留最新一份" : "保留全部"}</strong>
        <p>ZIP → {item.archive!.archive_dest}<br />解压目录 → {item.archive!.folder_dest}</p>
        {item.archive!.error && <p className="error" role="alert">{item.archive!.error}</p>}
        {!!(item.archive!.obsolete_sources.length + item.archive!.old_targets.length) && <details><summary>将暂存的旧包和目录（{item.archive!.obsolete_sources.length + item.archive!.old_targets.length} 项，可随整包撤销恢复）</summary><ul>{[...item.archive!.obsolete_sources, ...item.archive!.old_targets].map((path) => <li key={path}>{path}</li>)}</ul></details>}
        {item.archive!.cleanup_backup && <p className="muted">本次更新成功后，更早的撤销备份将进入回收站；仅保留本次更新的整包撤销能力。</p>}
      </div>)}
      <div className="table-scroll"><table className="file-table preview-table"><colgroup><col className="selection-col" /><col className="file-col" /><col className="action-col" /><col /></colgroup><thead><tr><th>
        <input type="checkbox" aria-label="全选文件" checked={count > 0 && count === eligible.length} aria-checked={count > 0 && count < eligible.length ? "mixed" : count > 0 && count === eligible.length} ref={(node) => { if (node) node.indeterminate = count > 0 && count < eligible.length; }} disabled={running || !eligible.length} onChange={() => setSelected(count === eligible.length ? new Set() : new Set(eligible.map((file) => file.key)))} />
      </th><th>文件</th><th>操作</th><th>目标位置</th></tr></thead><tbody>
        {files.map((file) => <Fragment key={file.key}>{file.operations.map((item, index) => <tr key={index} className={selected.has(file.key) ? "" : "excluded"}>
          {index === 0 && <><td rowSpan={file.operations.length}><input type="checkbox" aria-label={"选择文件：" + file.src} checked={selected.has(file.key)} disabled={running || file.operations.some((operation) => operation.archive?.error)} onChange={() => setSelected((current) => {
            const next = new Set(current); if (next.has(file.key)) next.delete(file.key); else next.add(file.key); return next;
          })} /></td><td rowSpan={file.operations.length} title={file.src}>{fileNameOf(file.src)}</td></>}
          <td>{KIND[item.kind] ?? item.kind}</td><td title={item.dest ?? item.action_desc}>{item.dest ?? item.action_desc}</td>
        </tr>)}</Fragment>)}
      </tbody></table></div>
      <details className="path-details"><summary>查看完整来源与目标路径</summary>{items.map((item, index) => <p key={index}>{item.src}<br />→ {item.dest ?? item.action_desc}</p>)}</details>
      <p className="hint"><IconShield /><span>{hasArchive ? "先成功解压，再替换旧版。包更新整组撤销；新 ZIP 或解压内容变化后停止撤销。" : hasRecycle ? "回收站中的文件可在系统回收站恢复。" : "操作会记录在日志中，可撤销最近一批整理。"} 自动规则仍按原条件运行。</span></p>
    </>}
    {error && <p className="error" role="alert">{error}</p>}
  </Modal>;
}
