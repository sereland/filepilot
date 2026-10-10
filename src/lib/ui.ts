export type Notify = (message: string, options?: { error?: boolean; action?: { label: string; onClick: () => void } }) => void;

export function uniqueFileCount(items: { src: string }[]) {
  return new Set(items.map((item) => sourceKey(item.src))).size;
}

export const sourceKey = (src: string) => src.replace(/\//g, "\\").toLowerCase();

export function formatLastRun(timestamp: string) {
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "已有执行记录";
  return "最近执行 " + new Intl.DateTimeFormat("zh-CN", {
    year: date.getFullYear() !== new Date().getFullYear() ? "numeric" : undefined,
    month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false,
  }).format(date);
}
