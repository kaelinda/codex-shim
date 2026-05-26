import type { AuthSnapshot, HealthSnapshot, TabKey } from "../types";
import Icon, { type IconName } from "./Icon";

interface Props {
  active: TabKey;
  onSelect: (tab: TabKey) => void;
  health: HealthSnapshot | null;
  auth: AuthSnapshot | null;
}

const TABS: { key: TabKey; label: string; icon: IconName; hint: string }[] = [
  { key: "dashboard", label: "Dashboard", icon: "dashboard", hint: "服务总览" },
  { key: "models", label: "Models", icon: "models", hint: "编辑 models.json" },
  { key: "active", label: "Active", icon: "active", hint: "切换 Codex 默认 model" },
  { key: "logs", label: "Logs", icon: "logs", hint: "查看 shim.log" },
  { key: "settings", label: "Settings", icon: "settings", hint: "CLI / 端口 / 路径" },
];

export default function Sidebar({ active, onSelect, health, auth }: Props) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <div className="brand-mark">CS</div>
        <div>
          <div className="brand-title">Codex Shim</div>
          <div className="brand-sub">Control Panel</div>
        </div>
      </div>
      <nav>
        {TABS.map((tab) => (
          <button
            key={tab.key}
            type="button"
            className={`nav-item ${active === tab.key ? "nav-item-active" : ""}`}
            onClick={() => onSelect(tab.key)}
          >
            <span className="nav-icon"><Icon name={tab.icon} /></span>
            <span>
              <div className="nav-label">{tab.label}</div>
              <div className="nav-hint">{tab.hint}</div>
            </span>
          </button>
        ))}
      </nav>
      <div className="sidebar-footer">
        <div className={`dot ${health?.ok ? "dot-ok" : "dot-bad"}`} />
        <span>daemon {health?.ok ? "running" : "stopped"}</span>
        <div className={`dot ${auth?.passthrough_available ? "dot-ok" : "dot-warn"}`} />
        <span>codex {auth?.passthrough_available ? "logged in" : "无 token"}</span>
      </div>
    </aside>
  );
}
