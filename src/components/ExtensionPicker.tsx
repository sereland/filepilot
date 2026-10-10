import { useState } from "react";
import Modal from "./Modal";
import { IconChevron, IconSearch } from "./icons";

const GROUPS = [
  { name: "图片", values: ["png", "jpg", "jpeg", "gif", "webp", "svg", "heic"] },
  { name: "文档", values: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md"] },
  { name: "压缩包", values: ["zip", "rar", "7z", "tar", "gz"] },
  { name: "音视频", values: ["mp4", "mov", "mkv", "mp3", "wav", "flac"] },
];
const normalize = (values: string[]) => [...new Set(values.map((value) => value.trim().replace(/^\.+/, "").toLowerCase()).filter(Boolean))];

export default function ExtensionPicker({ values, onChange }: { values: string[]; onChange: (values: string[]) => void }) {
  const [open, setOpen] = useState(false);
  const selected = normalize(values);
  const label = selected.length ? selected.slice(0, 3).map((ext) => `.${ext}`).join("、") + (selected.length > 3 ? ` 等 ${selected.length} 种` : "") : "选择文件类型";
  return <>
    <button type="button" className="type-select" aria-haspopup="dialog" onClick={() => setOpen(true)} title={selected.map((ext) => `.${ext}`).join("、")}>
      <span className={selected.length ? "type-value" : "type-value placeholder"}>{label}</span><IconChevron />
    </button>
    {open && <PickerDialog initial={selected} onClose={() => setOpen(false)} onConfirm={(next) => { onChange(next); setOpen(false); }} />}
  </>;
}

function PickerDialog({ initial, onClose, onConfirm }: { initial: string[]; onClose: () => void; onConfirm: (values: string[]) => void }) {
  const [selected, setSelected] = useState(initial);
  const [query, setQuery] = useState("");
  const [custom, setCustom] = useState("");
  const [error, setError] = useState("");
  const known = new Set(GROUPS.flatMap((group) => group.values));
  const groups = [...GROUPS, { name: "自定义", values: selected.filter((value) => !known.has(value)) }]
    .map((group) => ({ ...group, values: group.values.filter((ext) => ext.includes(query.trim().replace(/^\./, "").toLowerCase()) || group.name.includes(query.trim())) })).filter((group) => group.values.length);
  const add = () => {
    const ext = custom.trim().replace(/^\.+/, "").toLowerCase();
    if (!/^[a-z0-9][a-z0-9_-]{0,19}$/.test(ext)) return setError("输入一种扩展名，例如 heic；使用字母或数字，不含空格。");
    setSelected((current) => [...new Set([...current, ext])]); setCustom(""); setQuery(""); setError("");
  };
  return <Modal compact title="选择文件类型" description="可选择多种扩展名，匹配其中任一种即可。" onClose={onClose}
    footer={<><span className="foot-note">已选择 {selected.length} 种扩展名</span><button className="btn btn-secondary" onClick={onClose}>取消</button><button className="btn btn-primary" onClick={() => onConfirm(selected)}>完成选择</button></>}>
    <label className="search full"><IconSearch /><input aria-label="搜索扩展名" placeholder="搜索扩展名，如 PDF 或 JPG" value={query} onChange={(e) => setQuery(e.target.value)} /></label>
    {groups.map((group) => <div key={group.name}>
      <div className="ext-group-title"><span>{group.name}</span><button className="text-btn" onClick={() => setSelected((current) => group.values.every((ext) => current.includes(ext)) ? current.filter((ext) => !group.values.includes(ext)) : [...new Set([...current, ...group.values])])}>{group.values.every((ext) => selected.includes(ext)) ? "取消该组" : "选择该组"}</button></div>
      <div className="ext-options">{group.values.map((ext) => <label key={ext}><input type="checkbox" checked={selected.includes(ext)} onChange={(e) => setSelected((current) => e.target.checked ? [...new Set([...current, ext])] : current.filter((value) => value !== ext))} />.{ext}</label>)}</div>
    </div>)}
    {groups.length === 0 && <p className="muted small">没有找到该扩展名，可以在下方自定义添加。</p>}
    <div className="custom-extension"><input aria-label="自定义扩展名" placeholder="自定义扩展名，如 heic" value={custom} onChange={(e) => setCustom(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); add(); } }} /><button className="btn btn-secondary" onClick={add}>添加</button></div>
    {error && <p className="error" role="alert">{error}</p>}
  </Modal>;
}
