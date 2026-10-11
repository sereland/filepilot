interface Props {
  monitoring: boolean; active: boolean; busy: boolean; error: string;
  onToggle: () => void; onCheckRules: () => void;
}

export default function MonitoringStatus({ monitoring, active, busy, error, onToggle, onCheckRules }: Props) {
  const action = monitoring ? "暂停自动整理" : "恢复自动整理";
  return <div className="monitor-card">
    <div className="monitor-info">
      <div className="monitor-title">自动整理</div>
      <div className="monitor-state" role="status"><span className={"status-dot" + (monitoring ? " on" : "")} />{monitoring ? "运行中" : active ? "已暂停" : "暂无自动规则"}</div>
    </div>
    <button className="btn btn-secondary btn-small monitor-button" aria-label={action} title={!monitoring && !active ? "请先为一条规则开启自动整理" : action} disabled={busy || (!monitoring && !active)} onClick={onToggle}>{busy ? "处理中…" : monitoring ? "暂停" : "恢复"}</button>
    {error && <div className="monitor-error" role="alert"><span>{error}</span><button className="text-btn" onClick={onCheckRules}>检查规则</button></div>}
  </div>;
}
