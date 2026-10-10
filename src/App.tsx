import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import "./App.css";
import { IconBrand, IconCheck, IconHistory, IconLayers, IconSettings, IconTemplate, IconX } from "./components/icons";
import RuleEditor from "./components/RuleEditor";
import { api } from "./lib/api";
import { initTheme } from "./lib/theme";
import type { OpRecord, Rule } from "./lib/types";
import type { Notify } from "./lib/ui";
import OplogView from "./views/OplogView";
import RulesView from "./views/RulesView";
import SettingsView from "./views/SettingsView";
import TemplatesView from "./views/TemplatesView";

type Tab = "rules" | "templates" | "oplog" | "settings";
type Notice = { message: string; error?: boolean; action?: { label: string; onClick: () => void } };

export default function App() {
  const [tab, setTab] = useState<Tab>("rules");
  const [rules, setRules] = useState<Rule[]>([]);
  const [records, setRecords] = useState<OpRecord[]>([]);
  const [monitoring, setMonitoring] = useState(false);
  const [monitorBusy, setMonitorBusy] = useState(false);
  const [monitorError, setMonitorError] = useState("");
  const [editing, setEditing] = useState<Rule | null | undefined>();
  const [notice, setNotice] = useState<Notice | null>(null);
  const notify: Notify = useCallback((message, options) => {
    setNotice({ message, ...options });
  }, []);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), notice.error ? 12000 : 6000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const reload = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const [nextRules, nextRecords, running] = await Promise.all([api.getRules(), api.getOplog(100), api.isMonitoring()]);
      setRules(nextRules); setRecords(nextRecords); setMonitoring(running);
    } catch (e) { notify("无法加载应用数据：" + String(e), { error: true }); }
  }, [notify]);
  useEffect(() => {
    const cleanupTheme = initTheme();
    void reload();
    let disposed = false;
    const cleanups: (() => void)[] = [];
    if (isTauri()) {
      const subscribe = (promise: Promise<() => void>) => {
        promise.then((cleanup) => { if (disposed) cleanup(); else cleanups.push(cleanup); }).catch((e) => notify("无法接收状态更新：" + String(e), { error: true }));
      };
      subscribe(listen("fp://oplog-updated", () => { void reload(); }));
      subscribe(listen<boolean>("fp://monitoring-changed", (event) => setMonitoring(event.payload)));
    }
    return () => { disposed = true; cleanups.forEach((cleanup) => cleanup()); cleanupTheme?.(); };
  }, [reload, notify]);

  const active = rules.some((rule) => rule.enabled);
  const toggleMonitoring = async () => {
    setMonitorBusy(true); setMonitorError("");
    try {
      if (monitoring) { await api.stopMonitoring(); setMonitoring(false); }
      else {
        const running = await api.startMonitoring();
        setMonitoring(running);
        if (!running) setMonitorError(active ? "无法启动自动整理，请检查来源文件夹是否存在。" : "请先为一条规则开启自动整理。");
      }
    } catch (e) { setMonitorError("自动整理操作失败：" + String(e)); }
    finally { setMonitorBusy(false); }
  };
  const onSaved = async () => {
    setEditing(undefined); setTab("rules"); setMonitorError(""); await reload();
    notify("规则已保存，可先预览整理效果。");
  };
  const nav: { tab: Tab; label: string; icon: typeof IconLayers }[] = [
    { tab: "rules", label: "整理规则", icon: IconLayers }, { tab: "templates", label: "规则模板", icon: IconTemplate },
    { tab: "oplog", label: "操作记录", icon: IconHistory }, { tab: "settings", label: "设置", icon: IconSettings },
  ];
  return <div className="app">
    <aside className="sidebar">
      <div className="brand"><span className="brand-tile"><IconBrand /></span><span className="brand-name">FilePilot</span></div>
      <nav aria-label="主导航">{nav.map((item) => <button key={item.tab} className={"nav-item" + (tab === item.tab ? " active" : "")} aria-current={tab === item.tab ? "page" : undefined} onClick={() => setTab(item.tab)}><item.icon />{item.label}</button>)}</nav>
      <div className="sidebar-footer"><div className="monitor-card">
        <div className="monitor-title"><span className={"status-dot" + (monitoring ? " on" : "")} /><span>{monitoring ? "自动整理运行中" : active ? "自动整理已暂停" : "暂无自动规则"}</span></div>
        {monitorError && <div className="monitor-error" role="alert"><span>{monitorError}</span><button className="text-btn" onClick={() => { setTab("rules"); setMonitorError(""); }}>检查规则 →</button></div>}
        <button className="btn btn-secondary monitor-button" disabled={monitorBusy || (!monitoring && !active)} onClick={toggleMonitoring}>{monitorBusy ? "处理中…" : monitoring ? "暂停自动整理" : "恢复自动整理"}</button>
      </div></div>
    </aside>
    <main className="content">
      {tab === "rules" && <RulesView rules={rules} monitoring={monitoring} onReload={reload} onEdit={setEditing} onTemplates={() => setTab("templates")} onLogs={() => setTab("oplog")} notify={notify} />}
      {tab === "templates" && <TemplatesView onUse={setEditing} />}
      {tab === "oplog" && <OplogView records={records} onReload={reload} notify={notify} />}
      {tab === "settings" && <SettingsView notify={notify} />}
    </main>
    {editing !== undefined && <RuleEditor initial={editing} monitoring={monitoring} onClose={() => setEditing(undefined)} onSaved={onSaved} />}
    {notice && <div className={"toast" + (notice.error ? " error-notice" : "")} role={notice.error ? "alert" : "status"}>
      {!notice.error && <IconCheck />}<span>{notice.message}</span>
      {notice.action && <button className="text-btn" onClick={() => { notice.action?.onClick(); setNotice(null); }}>{notice.action.label}</button>}
      <button className="icon-btn" aria-label="关闭提示" onClick={() => setNotice(null)}><IconX /></button>
    </div>}
  </div>;
}
