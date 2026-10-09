import { useEffect, useState } from "react";
import { api } from "../lib/api";
import {
  type Theme,
  applyTheme,
  getEffectiveTheme,
  loadTheme,
  saveTheme,
  watchSystemTheme,
} from "../lib/theme";

export default function SettingsView() {
  const [autostart, setAutostart] = useState(false);
  const [theme, setTheme] = useState<Theme>(loadTheme());

  useEffect(() => {
    api
      .isAutostartEnabled()
      .then(setAutostart)
      .catch(() => {});
  }, []);

  useEffect(() => {
    // 监听系统主题变化（仅在 auto 模式下）
    if (theme === "auto") {
      return watchSystemTheme((systemTheme) => {
        applyTheme(systemTheme);
      });
    }
  }, [theme]);

  const toggleAutostart = async () => {
    try {
      setAutostart(await api.setAutostart(!autostart));
    } catch (e) {
      alert(`设置失败：${e}`);
    }
  };

  const changeTheme = (newTheme: Theme) => {
    setTheme(newTheme);
    saveTheme(newTheme);
    const effectiveTheme = getEffectiveTheme(newTheme);
    applyTheme(effectiveTheme);
  };

  return (
    <div>
      <div className="page-header">
        <div>
          <h2>设置</h2>
          <p className="page-sub">应用偏好</p>
        </div>
      </div>
      <div className="settings-list">
        <div className="settings-row">
          <div>
            <div className="settings-title">外观</div>
            <div className="muted small">选择浅色或深色主题</div>
          </div>
          <div style={{ display: "flex", gap: "8px" }}>
            <button
              className={`btn ${theme === "auto" ? "btn-primary" : "btn-secondary"}`}
              onClick={() => changeTheme("auto")}
            >
              跟随系统
            </button>
            <button
              className={`btn ${theme === "light" ? "btn-primary" : "btn-secondary"}`}
              onClick={() => changeTheme("light")}
            >
              浅色
            </button>
            <button
              className={`btn ${theme === "dark" ? "btn-primary" : "btn-secondary"}`}
              onClick={() => changeTheme("dark")}
            >
              深色
            </button>
          </div>
        </div>
        <div className="settings-row">
          <div>
            <div className="settings-title">开机自启</div>
            <div className="muted small">登录 Windows 后自动启动 FilePilot</div>
          </div>
          <label className="switch" title="开机自启">
            <input type="checkbox" checked={autostart} onChange={toggleAutostart} />
            <span className="slider" />
          </label>
        </div>
      </div>
    </div>
  );
}
