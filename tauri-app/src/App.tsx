import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import Dashboard from "./components/Dashboard";
import ModelsManager from "./components/ModelsManager";
import ActiveModel from "./components/ActiveModel";
import LogViewer from "./components/LogViewer";
import SettingsPanel from "./components/SettingsPanel";
import Sidebar from "./components/Sidebar";
import StatusBar from "./components/StatusBar";
import ErrorBoundary from "./components/ErrorBoundary";
import { useAdaptivePolling } from "./hooks/useAdaptivePolling";
import type {
  AppSettingsDto,
  AuthSnapshot,
  HealthSnapshot,
  RuntimeInfo,
  TabKey,
} from "./types";

const TAB_TITLES: Record<TabKey, string> = {
  dashboard: "Dashboard",
  models: "Models",
  active: "Active model",
  logs: "Logs",
  settings: "Settings",
};

export default function App() {
  const [tab, setTab] = useState<TabKey>("dashboard");
  const [runtime, setRuntime] = useState<RuntimeInfo | null>(null);
  const [settings, setSettings] = useState<AppSettingsDto | null>(null);
  const [health, setHealth] = useState<HealthSnapshot | null>(null);
  const [auth, setAuth] = useState<AuthSnapshot | null>(null);
  const [activeModel, setActiveModel] = useState<string | null>(null);
  const [toast, setToast] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const toastTimerRef = useRef<number | null>(null);

  const refreshSettings = useCallback(async () => {
    const next = await api.appSettings();
    setSettings(next);
    return next;
  }, []);

  const refreshAll = useCallback(async () => {
    const tasks = await Promise.allSettled([
      api.health(),
      api.readCodexAuth(),
      api.currentActiveModel(),
    ]);
    if (tasks[0].status === "fulfilled") setHealth(tasks[0].value);
    if (tasks[1].status === "fulfilled") setAuth(tasks[1].value);
    if (tasks[2].status === "fulfilled") setActiveModel(tasks[2].value);
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const [rt] = await Promise.all([api.runtimeInfo(), refreshSettings()]);
        setRuntime(rt);
        await refreshAll();
      } catch (e) {
        setBootError(describeError(e));
      }
    })();
  }, [refreshAll, refreshSettings]);

  useAdaptivePolling(refreshAll, { intervalMs: 5000 });

  const flash = useCallback((kind: "ok" | "err", text: string) => {
    if (toastTimerRef.current !== null) {
      window.clearTimeout(toastTimerRef.current);
    }
    setToast({ kind, text });
    toastTimerRef.current = window.setTimeout(() => {
      setToast(null);
      toastTimerRef.current = null;
    }, 3200);
  }, []);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current !== null) {
        window.clearTimeout(toastTimerRef.current);
      }
    };
  }, []);

  const handleAction = useCallback(
    async (label: string, action: () => Promise<unknown>) => {
      try {
        await action();
        flash("ok", `${label} 成功`);
        await refreshAll();
      } catch (e) {
        flash("err", `${label} 失败: ${describeError(e)}`);
      }
    },
    [flash, refreshAll],
  );

  const body = useMemo(() => {
    if (bootError) {
      return (
        <div className="error-card" role="alert">
          <h2>初始化失败</h2>
          <pre>{bootError}</pre>
        </div>
      );
    }
    if (!runtime || !settings) {
      return <div className="spinner" role="status">加载中…</div>;
    }
    switch (tab) {
      case "dashboard":
        return (
          <Dashboard
            runtime={runtime}
            settings={settings}
            health={health}
            auth={auth}
            activeModel={activeModel}
            onAction={handleAction}
            flash={flash}
          />
        );
      case "models":
        return <ModelsManager flash={flash} />;
      case "active":
        return (
          <ActiveModel
            health={health}
            auth={auth}
            activeModel={activeModel}
            onUseModel={(slug) =>
              handleAction(`切换到 ${slug}`, () => api.useModel(slug))
            }
            flash={flash}
          />
        );
      case "logs":
        return <LogViewer logPath={runtime.log_path} />;
      case "settings":
        return (
          <SettingsPanel
            runtime={runtime}
            settings={settings}
            onUpdated={async (next) => {
              setSettings(next);
              await refreshAll();
            }}
            flash={flash}
          />
        );
    }
  }, [activeModel, auth, bootError, flash, handleAction, health, refreshAll, runtime, settings, tab]);

  return (
    <div className="app">
      <Sidebar
        active={tab}
        onSelect={setTab}
        health={health}
        auth={auth}
      />
      <main className="main">
        <header className="topbar">
          <h1 id="page-title">{TAB_TITLES[tab]}</h1>
          <div className="topbar-meta">
            <span className={`pill ${health?.ok ? "pill-ok" : "pill-bad"}`}>
              {health?.ok ? `daemon ok · ${health.models ?? "?"} 个模型` : "daemon 离线"}
            </span>
            {auth?.passthrough_available && (
              <span className="pill pill-ok">ChatGPT passthrough 可用</span>
            )}
          </div>
        </header>
        <section className="content" aria-labelledby="page-title">
          <ErrorBoundary resetKey={tab}>{body}</ErrorBoundary>
        </section>
      </main>
      <StatusBar runtime={runtime} settings={settings} />
      {toast && (
        <div
          className={`toast toast-${toast.kind}`}
          role={toast.kind === "err" ? "alert" : "status"}
          aria-live={toast.kind === "err" ? "assertive" : "polite"}
          aria-atomic="true"
        >
          {toast.text}
        </div>
      )}
    </div>
  );
}

function describeError(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message?: unknown }).message ?? err);
  }
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}
