import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export type ProjectReport = {
  project_id: string;
  name: string;
  total_tasks: number;
  completed_tasks: number;
  failed_tasks: number;
  cancelled_tasks: number;
  queued_tasks: number;
  active_tasks: number;
  completion_rate: number;
  execution_attempts: number;
  retries: number;
  verification_passed: number;
  verification_failed: number;
  token_input: number | null;
  token_output: number | null;
  token_total: number | null;
  recent_failures: string[];
  recent_tasks: string[];
};

type ProjectRecord = {
  id: string;
  name: string;
  root: string;
  provider_profile_id: string | null;
  created_at: string;
  last_opened_at: string;
  pinned: boolean;
  archived?: boolean;
};

const STATUS_COLORS = {
  completed: "#73c69c",
  failed: "#ff8a8a",
  cancelled: "#f5c842",
  queued: "#9aa0ad",
  active: "#6aa6ff",
};

function formatTokenCount(value: number | null): string {
  if (value == null) return "Unavailable";
  return value.toLocaleString();
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

export function ProjectReportModal({
  isOpen,
  onClose,
  projectId,
  projects,
  onRefresh,
}: {
  isOpen: boolean;
  onClose: () => void;
  projectId: string;
  projects: ProjectRecord[];
  onRefresh?: () => void;
}) {
  const [report, setReport] = useState<ProjectReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen && projectId) {
      setLoading(true);
      setError(null);
      invoke<ProjectReport>("get_project_report", { projectId })
        .then(setReport)
        .catch((e) => setError(typeof e === "string" ? e : "Failed to load project report"))
        .finally(() => setLoading(false));
    }
  }, [isOpen, projectId]);

  const project = projects.find((p) => p.id === projectId);

  if (!isOpen) return null;

  if (loading) {
    return (
      <div className="config-modal-backdrop" onClick={onClose}>
        <div className="config-modal report-modal" style={{ width: "min(800px, 96vw)", maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()}>
          <div className="config-modal-header">
            <h2>Project Report</h2>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
          <div className="config-modal-body" style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>
            Loading project report…
          </div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="config-modal-backdrop" onClick={onClose}>
        <div className="config-modal report-modal" style={{ width: "min(800px, 96vw)", maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()}>
          <div className="config-modal-header">
            <h2>Project Report</h2>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
          <div className="config-modal-body" style={{ padding: 24 }}>
            <div className="inline-error">{error}</div>
            <button className="primary-button" onClick={() => window.location.reload()} style={{ marginTop: 12 }}>Retry</button>
          </div>
        </div>
      </div>
    );
  }

  if (!report) return null;

  const completionRatePercent = (report.completion_rate * 100).toFixed(1);

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal report-modal"
        onClick={(e) => e.stopPropagation()}
        style={{ width: "min(800px, 96vw)", maxHeight: "90vh", display: "flex", flexDirection: "column" }}
      >
        <div className="config-modal-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
          <div>
            <h2 style={{ margin: 0, display: "flex", alignItems: "center", gap: 8 }}>
              Project Report
              <span style={{ fontSize: 12, color: "#9aa0ad" }}>{project?.name || report.name}</span>
            </h2>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="secondary-button" onClick={() => { if (onRefresh) onRefresh(); }} disabled={loading}>
              {loading ? "Refreshing..." : "Refresh"}
            </button>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
        </div>

        <div className="config-modal-body" style={{ flex: 1, overflowY: "auto", padding: 16 }}>
          {/* Summary Cards */}
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 12, marginBottom: 24 }}>
            <ReportCard title="Total Tasks" value={report.total_tasks} color="#e3e5ec" />
            <ReportCard title="Completed" value={report.completed_tasks} color={STATUS_COLORS.completed} />
            <ReportCard title="Failed" value={report.failed_tasks} color={STATUS_COLORS.failed} />
            <ReportCard title="Cancelled" value={report.cancelled_tasks} color={STATUS_COLORS.cancelled} />
            <ReportCard title="Queued" value={report.queued_tasks} color={STATUS_COLORS.queued} />
            <ReportCard title="Active" value={report.active_tasks} color={STATUS_COLORS.active} />
            <ReportCard title="Completion Rate" value={`${completionRatePercent}%`} color="#f5c842" />
            <ReportCard title="Attempts" value={report.execution_attempts} color="#6aa6ff" />
            <ReportCard title="Retries" value={report.retries} color="#f5c842" />
          </div>

          {/* Verification Summary */}
          <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 24 }}>
            <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Verification Summary</h3>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 12 }}>
              <div style={{ textAlign: "center" }}>
                <div style={{ fontSize: 28, fontWeight: 600, color: "#73c69c" }}>{report.verification_passed}</div>
                <div style={{ fontSize: 11, color: "#9aa0ad" }}>Passed</div>
              </div>
              <div style={{ textAlign: "center" }}>
                <div style={{ fontSize: 28, fontWeight: 600, color: "#ff8a8a" }}>{report.verification_failed}</div>
                <div style={{ fontSize: 11, color: "#9aa0ad" }}>Failed</div>
              </div>
              <div style={{ textAlign: "center" }}>
                <div style={{ fontSize: 28, fontWeight: 600, color: "#e3e5ec" }}>{report.verification_passed + report.verification_failed}</div>
                <div style={{ fontSize: 11, color: "#9aa0ad" }}>Total Runs</div>
              </div>
            </div>
          </div>

          {/* Token Usage */}
          <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 24 }}>
            <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Aggregate Token Usage</h3>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 12 }}>
              <div style={{ textAlign: "center" }}>
                <div style={{ fontSize: 24, fontWeight: 600, color: "#6aa6ff" }}>{formatTokenCount(report.token_input)}</div>
                <div style={{ fontSize: 11, color: "#9aa0ad" }}>Input Tokens</div>
              </div>
              <div style={{ textAlign: "center" }}>
                <div style={{ fontSize: 24, fontWeight: 600, color: "#73c69c" }}>{formatTokenCount(report.token_output)}</div>
                <div style={{ fontSize: 11, color: "#9aa0ad" }}>Output Tokens</div>
              </div>
              <div style={{ textAlign: "center" }}>
                <div style={{ fontSize: 24, fontWeight: 600, color: "#f5c842" }}>{formatTokenCount(report.token_total)}</div>
                <div style={{ fontSize: 11, color: "#9aa0ad" }}>Total Tokens</div>
              </div>
            </div>
          </div>

          {/* Recent Failures */}
          {report.recent_failures.length > 0 && (
            <div style={{ background: "#2d1111", border: "1px solid #ff8a8a44", borderRadius: 8, padding: 16, marginBottom: 24 }}>
              <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#ff8a8a" }}>Recent Failures ({report.recent_failures.length})</h3>
              <ul style={{ margin: 0, paddingLeft: 20, color: "#ff8a8a" }}>
                {report.recent_failures.map((taskId, idx) => (
                  <li key={idx} style={{ marginBottom: 4, fontFamily: "monospace", fontSize: 12 }}>
                    {shortId(taskId)}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Recent Tasks */}
          {report.recent_tasks.length > 0 && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16 }}>
              <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Recent Tasks ({report.recent_tasks.length})</h3>
              <ul style={{ margin: 0, paddingLeft: 20, color: "#b4b8c6" }}>
                {report.recent_tasks.map((taskId, idx) => (
                  <li key={idx} style={{ marginBottom: 4, fontFamily: "monospace", fontSize: 12 }}>
                    {shortId(taskId)}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {report.recent_failures.length === 0 && report.recent_tasks.length === 0 && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 24, textAlign: "center", color: "#9aa0ad" }}>
              No recent task activity recorded for this project.
            </div>
          )}
        </div>

        <div className="config-modal-footer" style={{ display: "flex", justifyContent: "flex-end", gap: 8, paddingTop: 12, borderTop: "1px solid #2f3542" }}>
          <button className="secondary-button" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}

function ReportCard({ title, value, color }: { title: string; value: string | number; color: string }) {
  return (
    <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, textAlign: "center", borderTop: `3px solid ${color}` }}>
      <div style={{ fontSize: 28, fontWeight: 600, color }}>{value}</div>
      <div style={{ fontSize: 11, color: "#9aa0ad", marginTop: 4 }}>{title}</div>
    </div>
  );
}