import { useState } from "react";
import { Button } from "../components/Button";

interface AutomationRunDetailsScreenProps {
  runId: string;
}

/**
 * Automation Run Details screen — design.md §18.7.
 *
 * Full detail view of a single automation run: output, exit code, and
 * timing. Automation backend commands are not yet exposed by the Tauri
 * core; this screen shows a graceful placeholder.
 */
export function AutomationRunDetailsScreen({ runId }: AutomationRunDetailsScreenProps) {
  const [error, setError] = useState<string | null>(null);

  return (
    <div className="automation-run-details-screen" role="main" aria-label="Automation Run Details">
      <header className="automation-run-details-head">
        <div>
          <h2 className="automation-run-details-title">Automation Run Details</h2>
          <span className="automation-run-details-id">Run #{runId}</span>
        </div>
      </header>

      {error && (
        <div className="automation-run-details-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="automation-run-details-section automation-run-details-section-placeholder">
        <h3 className="automation-run-details-section-title">Automation Run Backend</h3>
        <div className="automation-run-details-coming-soon">
          <span className="automation-run-details-coming-soon-icon">▱</span>
          <p className="automation-run-details-coming-soon-title">Automation run backend integration</p>
          <p className="automation-run-details-coming-soon-desc">
            The automation run backend (get_automation_run) is not yet
            exposed by the Tauri core. Full run output, exit code, and
            timing details will be available in a later milestone
            (design.md §18.7).
          </p>
        </div>
      </div>
    </div>
  );
}

export default AutomationRunDetailsScreen;
