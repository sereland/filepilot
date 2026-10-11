import type { ExecutionSummary } from "../lib/executionSummary";
import { executionStatusLabel } from "../lib/executionSummary";
import { countLabel, formatExecutionTime } from "../lib/ui";

export default function RecentExecution({ summary, onOpen }: {
  summary: ExecutionSummary | null | undefined; onOpen: (batchId: string) => void;
}) {
  if (!summary) return <div className="recent-run recent-run-empty">
    <span className="recent-run-header"><span className="recent-run-heading">最近</span><span className="recent-run-name">{summary === undefined ? "执行状态未加载" : "暂未执行"}</span></span>
  </div>;
  const time = formatExecutionTime(summary.timestamp);
  const count = countLabel(summary.fileCount + summary.directoryCount, summary.directoryCount);
  const status = executionStatusLabel(summary);
  const failure = summary.status === "failed" || summary.status === "partial_failure";
  const details = count + (summary.failedCount && summary.status !== "failed" ? ` · ${summary.failedCount} 项失败` : "");
  return <button className="recent-run recent-run-link" onClick={() => onOpen(summary.batchId)}
    aria-label={`查看最近一次整理记录：${time}，${summary.ruleName}，${details}，${status}`}
    title={`${summary.ruleName}\n${time} · ${details} · ${status}\n点击查看对应批次记录`}>
    <span className="recent-run-header"><span className="recent-run-heading">最近</span><span className="recent-run-name">{summary.ruleName}</span></span>
    <span className="recent-run-meta"><span>{time}</span><span aria-hidden="true">·</span><span>{count}</span></span>
    {(summary.status !== "completed" || summary.warningCount > 0) && <span className={"recent-run-status" + (failure ? " failure" : "")}>{status}{summary.failedCount && summary.status !== "failed" ? ` · ${summary.failedCount} 项失败` : ""}</span>}
  </button>;
}
