import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { version } from "../../package.json";
import Switch from "../components/Switch";
import { api } from "../lib/api";
import { type Theme, applyTheme, getEffectiveTheme, loadTheme, saveTheme } from "../lib/theme";
import type { Notify } from "../lib/ui";

export default function SettingsView({ notify }: { notify: Notify }) {
  const [autostart, setAutostart] = useState(false);
  const [theme, setTheme] = useState<Theme>(loadTheme);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!isTauri()) return;
    api.isAutostartEnabled().then(setAutostart).catch((e) => notify("无法读取启动设置：" + String(e), { error: true }));
  }, [notify]);
  const changeTheme = (next: Theme) => {
    try { saveTheme(next); setTheme(next); applyTheme(getEffectiveTheme(next)); }
    catch (e) { notify("无法保存主题设置：" + String(e), { error: true }); }
  };
  const toggleAutostart = async () => {
    setBusy(true);
    try { setAutostart(await api.setAutostart(!autostart)); }
    catch (e) { notify("启动设置失败：" + String(e), { error: true }); }
    finally { setBusy(false); }
  };
  return <div>
    <header className="page-header"><div><h1>设置</h1><p className="page-sub">让 FilePilot 按照你的习惯工作。</p></div></header>
    <h2 className="section-label">外观与启动</h2>
    <div className="settings-list">
      <div className="settings-row"><div><strong>外观主题</strong><p>选择你喜欢的工作环境</p></div><div className="segmented" role="group" aria-label="外观主题">
        {([{ value: "auto", label: "跟随系统" }, { value: "light", label: "浅色" }, { value: "dark", label: "深色" }] as const).map((item) => <button key={item.value} aria-pressed={theme === item.value} className={theme === item.value ? "active" : ""} onClick={() => changeTheme(item.value)}>{item.label}</button>)}
      </div></div>
      <div className="settings-row"><div><strong>开机启动</strong><p>登录 Windows 后自动启动 FilePilot</p></div><Switch checked={autostart} disabled={busy} label="开机启动" onChange={() => { void toggleAutostart(); }} /></div>
    </div>
    <h2 className="section-label">关于 FilePilot</h2><div className="settings-list"><div className="settings-row"><div><strong>FilePilot</strong><p>Windows 文件自动整理工具 · 纯本地运行</p></div><span className="muted small">版本 {version}</span></div></div>
  </div>;
}
