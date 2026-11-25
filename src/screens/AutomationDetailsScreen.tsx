import { useState } from "react";
import { Button } from "../components/Button";

interface AutomationDetailsScreenProps {
  automationId: string;
}

/**
 * Automation Details screen — design.md §18.
 *
 * Full detail view of a single automation: configuration, recent runs,
 * and enable/disable controls. Automation backend commands are not yet
 * exposed by the Tauri core; this screen shows a graceful placeholder
 * with the automation ID and a notice that full integration is coming
 * in a later milestone.
 */
export function AutomationDetailsScreen({ automationId }: AutomationDetailsScreenProps) {
  const [error, setError] = useState<string | null>(null);

  return (
    <div className="automation-details-screen" role="main" aria-label="Automation Details">
      <header className="automation-details-head">
        <div>
          <h2 className="automation-details-title">Automation Details</h2>
          <span className="automation-details-id">Automation #{automationId}</span>
        </div>
      </header>

      {error && (
        <div className="automation-details-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="automation-details-section automation-details-section-placeholder">
        <h3 className="automation-details-section-title">Automation Backend</h3>
        <div className="automation-details-coming-soon">
          <span className="automation-details-coming-soon-icon">▱</span>
          <p className="automation-details-coming-soon-title">Automation backend integration</p>
          <p className="automation-details-coming-soon-desc">
            The automation execution backend (list_automation_runs, get_automation)
            is not yet exposed by the Tauri core. Full automation detail,
            run history, and toggle controls will be available in a later
            milestone (design.md §18).
          </p>
        </div>
      </div>
    </div>
  );
}

export default AutomationDetailsScreen;
