import { useEffect, useState } from "react";
import { api } from "../lib/api";

export default function SettingsView() {
  const [autostart, setAutostart] = useState(false);

  useEffect(() => {
    api
      .isAutostartEnabled()
      .then(setAutostart)
      .catch(() => {});
  }, []);

  const toggleAutostart = async () => {
    try {
      setAutostart(await api.setAutostart(!autostart));
    } catch (e) {
      alert(`设置失败：${e}`);
    }
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
