import { fileNameOf, type PlanItem } from "../lib/types";

interface Props {
  ruleName: string;
  items: PlanItem[];
  running: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

/** dry-run 预览：先预览，再动手 */
export default function PreviewDialog({ ruleName, items, running, onConfirm, onClose }: Props) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>预览：{ruleName}</h2>
          <button className="icon-btn" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="modal-body">
          {items.length === 0 ? (
            <div className="empty">
              <p>没有文件符合这条规则。</p>
              <p className="muted small">可以先往监控文件夹里放几个测试文件再试。</p>
            </div>
          ) : (
            <>
              <p>
                这条规则将会处理 <strong>{items.length}</strong> 个文件：
              </p>
              <div className="plan-list">
                {items.map((it, i) => (
                  <div key={i} className="plan-item">
                    <span className="plan-src" title={it.src}>
                      {fileNameOf(it.src)}
                    </span>
                    <span className="plan-arrow">→</span>
                    <span className="plan-desc">{it.action_desc}</span>
                  </div>
                ))}
              </div>
              <div className="hint">
                👀 这只是预览，没有动你的文件。确认无误后再点"确认执行"。
              </div>
            </>
          )}
        </div>
        <div className="modal-footer">
          <button className="btn btn-secondary" onClick={onClose}>
            取消
          </button>
          <button
            className="btn btn-primary"
            onClick={onConfirm}
            disabled={items.length === 0 || running}
          >
            {running ? "执行中…" : `确认执行（${items.length} 个文件）`}
          </button>
        </div>
      </div>
    </div>
  );
}
