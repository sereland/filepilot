export type Notify = (message: string, options?: { error?: boolean; action?: { label: string; onClick: () => void } }) => void;

export function uniqueFileCount(items: { src: string }[]) {
  return new Set(items.map((item) => sourceKey(item.src))).size;
}

export const sourceKey = (src: string) => src.replace(/\//g, "\\").toLowerCase();

export function formatLastRun(timestamp: string, now = new Date()) {
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "上次整理：时间未知";
  const sameDay = (day: Date) => date.getFullYear() === day.getFullYear() && date.getMonth() === day.getMonth() && date.getDate() === day.getDate();
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  const time = new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false }).format(date);
  if (sameDay(now)) return "上次整理：今天 " + time;
  if (sameDay(yesterday)) return "上次整理：昨天 " + time;
  return "上次整理：" + new Intl.DateTimeFormat("zh-CN", {
    year: date.getFullYear() !== now.getFullYear() ? "numeric" : undefined,
    month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false,
  }).format(date);
}
