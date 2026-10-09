import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import "./App.css";
import { IconBrand, IconHistory, IconLayers } from "./components/icons";
import { api } from "./lib/api";
import type { OpRecord, Rule } from "./lib/types";
import OplogView from "./views/OplogView";
import RulesView from "./views/RulesView";

type Tab = "rules" | "oplog";

function App() {
  const [tab, setTab] = useState<Tab>("rules");
  const [rules, setRules] = useState<Rule[]>([]);
  const [records, setRecords] = useState<OpRecord[]>([]);
  const [monitoring, setMonitoring] = useState(false);
  const [autostart, setAutostart] = useState(false);

  const reload = useCallback(async () => {
    try {
      const [r, o, m, a] = await Promise.all([
        api.getRules(),
        api.getOplog(100),
        api.isMonitoring(),
        api.isAutostartEnabled(),
      ]);
      setRules(r);
      setRecords(o);
      setMonitoring(m);
      setAutostart(a);
    } catch {
      // 非 Tauri 环境（纯浏览器预览）时忽略
    }
  }, []);

  useEffect(() => {
    reload();
    let unlisten1: (() => void) | undefined;
    let unlisten2: (() => void) | undefined;
    listen("fp://oplog-updated", () => reload())
      .then((fn) => (unlisten1 = fn))
      .catch(() => {});
    listen<boolean>("fp://monitoring-changed", (e) => setMonitoring(e.payload))
      .then((fn) => (unlisten2 = fn))
      .catch(() => {});
    return () => {
      unlisten1?.();
      unlisten2?.();
    };
  }, [reload]);

  const toggleMonitoring = async () => {
    try {
      if (monitoring) {
        await api.stopMonitoring();
        setMonitoring(false);
      } else {
        const ok = await api.startMonitoring();
        setMonitoring(ok);
        if (!ok) alert("没有启用的规则或监控文件夹，无法启动监控");
      }
    } catch (e) {
      alert(`操作失败：${e}`);
    }
  };

  const toggleAutostart = async () => {
    try {
      const ok = await api.setAutostart(!autostart);
      setAutostart(ok);
    } catch (e) {
      alert(`设置失败：${e}`);
    }
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-tile">
            <IconBrand />
          </span>
          <div>
            <div className="brand-name">FilePilot</div>
            <div className="brand-sub">文件自动整理</div>
          </div>
        </div>
        <nav>
          <button
            className={`nav-item ${tab === "rules" ? "active" : ""}`}
            onClick={() => setTab("rules")}
          >
            <IconLayers />
            整理规则
          </button>
          <button
            className={`nav-item ${tab === "oplog" ? "active" : ""}`}
            onClick={() => setTab("oplog")}
          >
            <IconHistory />
            操作记录
          </button>
        </nav>
        <div className="sidebar-footer">
          <button
            className={`monitor-card ${monitoring ? "on" : "off"}`}
            onClick={toggleMonitoring}
            title="自动监控文件夹变化并执行规则"
          >
            <span className="status-dot" />
            <span>
              {monitoring ? "监控中" : "已暂停"}
              <span className="sub">{monitoring ? "规则自动运行" : "点击开启自动整理"}</span>
            </span>
          </button>
          <label className="autostart-row">
            <input type="checkbox" checked={autostart} onChange={toggleAutostart} />
            <span className="small">开机自启</span>
          </label>
          <div className="slogan">先预览 · 再动手 · 可撤销</div>
        </div>
      </aside>
      <main className="content">
        {tab === "rules" ? (
          <RulesView rules={rules} onReload={reload} />
        ) : (
          <OplogView records={records} onReload={reload} />
        )}
      </main>
    </div>
  );
}

export default App;
