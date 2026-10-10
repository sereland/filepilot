export type Notify = (message: string, options?: { error?: boolean; action?: { label: string; onClick: () => void } }) => void;

export function uniqueFileCount(items: { src: string }[]) {
  return new Set(items.map((item) => item.src.replace(/\//g, "\\").toLowerCase())).size;
}
