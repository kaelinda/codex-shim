import { useEffect, useRef } from "react";

interface PollingOptions {
  enabled?: boolean;
  intervalMs: number;
  pauseWhenHidden?: boolean;
}

export function useAdaptivePolling(
  callback: () => Promise<void> | void,
  { enabled = true, intervalMs, pauseWhenHidden = true }: PollingOptions,
) {
  const callbackRef = useRef(callback);

  useEffect(() => {
    callbackRef.current = callback;
  }, [callback]);

  useEffect(() => {
    if (!enabled) return;

    let cancelled = false;
    let inFlight = false;
    let timer: number | null = null;

    const clearTimer = () => {
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
    };

    const schedule = () => {
      if (!cancelled) {
        clearTimer();
        timer = window.setTimeout(run, intervalMs);
      }
    };

    const run = async () => {
      if (cancelled) return;
      if (pauseWhenHidden && document.hidden) {
        schedule();
        return;
      }
      if (inFlight) {
        schedule();
        return;
      }

      inFlight = true;
      try {
        await callbackRef.current();
      } catch {
        // Individual pollers own their UI error state; keep the loop alive.
      } finally {
        inFlight = false;
        schedule();
      }
    };

    const onVisibilityChange = () => {
      if (!document.hidden) {
        clearTimer();
        void run();
      }
    };

    document.addEventListener("visibilitychange", onVisibilityChange);
    schedule();

    return () => {
      cancelled = true;
      clearTimer();
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [enabled, intervalMs, pauseWhenHidden]);
}
