import { useState } from "react";
import { Button } from "../components/Button";

interface LiveAutomationMonitorScreenProps {
}

/**
 * Live Automation Monitor screen — design.md §18.7.
 *
 * Real-time view of all running automations with live status and output.
 * Automation backend commands are not yet exposed by the Tauri core;
 * this screen shows a graceful placeholder.
 */
export function LiveAutomationMonitorScreen({}: LiveAutomationMonitorScreenProps) {
  const [error, setError] = useState<string | null>(null);

  return (
    <div className="live-automation-monitor-screen" role="main" aria-label="Live Automation Monitor">
      <header className="live-automation-monitor-head">
        <div>
          <h2 className="live-automation-monitor-title">Live Automation Monitor</h2>
          <p className="live-automation-monitor-sub">Real-time automation status.</p>
        </div>
        <div className="live-automation-monitor-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => {}}
            title="Refresh automations"
          >
            Refresh
          </Button>
        </div>
      </header>

      {error && (
        <div className="live-automation-monitor-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="live-automation-monitor-section live-automation-monitor-section-placeholder">
        <h3 className="live-automation-monitor-section-title">Automation Monitor Backend</h3>
        <div className="live-automation-monitor-coming-soon">
          <span className="live-automation-monitor-coming-soon-icon">▱</span>
          <p className="live-automation-monitor-coming-soon-title">Live automation monitoring</p>
          <p className="live-automation-monitor-coming-soon-desc">
            The automation monitoring backend (list_automations, cancel_automation)
            is not yet exposed by the Tauri core. Real-time automation status,
            running automation lists, and cancel controls will be available
            in a later milestone (design.md §18.7).
          </p>
        </div>
      </div>
    </div>
  );
}

export default LiveAutomationMonitorScreen;
