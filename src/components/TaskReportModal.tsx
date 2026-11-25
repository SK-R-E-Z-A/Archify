import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export type TaskReport = {
  task_id: string;
  project_id: string | null;
  project_name: string | null;
  original_request: string;
  status: string;
  created_at: string;
  updated_at: string;
  elapsed_ms: number | null;
  planning_summary: string | null;
  agents: string[];
  models: string[];
  execution_attempts: number;
  verification_results: VerificationResult[];
  review_results: ReviewerResponse[];
  corrections: number;
  retries: number;
  files_changed: ChangedFiles;
  tests_and_commands: string[];
  final_outcome: string;
  remaining_issues: string[];
  token_input: number | null;
  token_output: number | null;
  token_total: number | null;
  known_facts: string[];
  unavailable: string[];
};

export type VerificationResult = {
  command: string;
  status: string;
  exit_code: number | null;
  output: string;
};

export type ReviewerResponse = {
  verdict: string;
  findings: string;
  unresolved_issues: string[];
  suggested_fix: string | null;
};

export type ChangedFiles = {
  created: string[];
  modified: string[];
  deleted: string[];
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

const STATUS_COLORS: Record<string, string> = {
  completed: "#73c69c",
  failed: "#ff8a8a",
  cancelled: "#f5c842",
  executing: "#6aa6ff",
  verifying: "#6aa6ff",
  reviewing: "#6aa6ff",
  retrying: "#f5c842",
  queued: "#9aa0ad",
  planning: "#6aa6ff",
  awaiting_approval: "#f5c842",
};

const VERIFICATION_STATUS_COLORS: Record<string, string> = {
  passed: "#73c69c",
  failed: "#ff8a8a",
  skipped: "#9aa0ad",
  unavailable: "#9aa0ad",
};

const REVIEW_VERDICT_COLORS: Record<string, string> = {
  complete: "#73c69c",
  needs_changes: "#f5c842",
  blocked: "#ff8a8a",
};

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatDuration(ms: number | null): string {
  if (ms == null) return "Unknown";
  if (ms < 1000) return `${ms} ms`;
  const secs = Math.floor(ms / 1000);
  const mins = Math.floor(secs / 60);
  const hours = Math.floor(mins / 60);
  if (hours > 0) return `${hours}h ${mins % 60}m`;
  if (mins > 0) return `${mins}m ${secs % 60}s`;
  return `${secs}s`;
}

function formatTokenCount(value: number | null): string {
  if (value == null) return "Unavailable";
  return value.toLocaleString();
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

export function TaskReportModal({
  isOpen,
  onClose,
  taskId,
  projects,
  onRefresh,
}: {
  isOpen: boolean;
  onClose: () => void;
  taskId: string;
  projects: ProjectRecord[];
  onRefresh?: () => void;
}) {
  const [report, setReport] = useState<TaskReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen && taskId) {
      setLoading(true);
      setError(null);
      invoke<TaskReport>("get_task_report", { taskId })
        .then(setReport)
        .catch((e) => setError(typeof e === "string" ? e : "Failed to load task report"))
        .finally(() => setLoading(false));
    }
  }, [isOpen, taskId]);

  const projectName = (id: string | null | undefined) => {
    if (!id) return "Unknown project";
    return projects.find((p) => p.id === id)?.name ?? id;
  };

  const getStatusLabel = (status: string) => {
    return status.charAt(0).toUpperCase() + status.slice(1).replace(/_/g, " ");
  };

  if (!isOpen) return null;

  if (loading) {
    return (
      <div className="config-modal-backdrop" onClick={onClose}>
        <div className="config-modal report-modal" style={{ width: "min(900px, 96vw)", maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()}>
          <div className="config-modal-header">
            <h2>Task Execution Report</h2>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
          <div className="config-modal-body" style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>
            Loading task report…
          </div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="config-modal-backdrop" onClick={onClose}>
        <div className="config-modal report-modal" style={{ width: "min(900px, 96vw)", maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()}>
          <div className="config-modal-header">
            <h2>Task Execution Report</h2>
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

  const statusColor = STATUS_COLORS[report.status] || "#6aa6ff";

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal report-modal"
        onClick={(e) => e.stopPropagation()}
        style={{ width: "min(900px, 96vw)", maxHeight: "90vh", display: "flex", flexDirection: "column" }}
      >
        <div className="config-modal-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
          <div>
            <h2 style={{ margin: 0, display: "flex", alignItems: "center", gap: 8 }}>
              Task Execution Report
              <span style={{ fontSize: 12, padding: "2px 8px", borderRadius: 12, background: `${statusColor}22`, color: statusColor, fontWeight: 600, textTransform: "capitalize" }}>
                {getStatusLabel(report.status)}
              </span>
            </h2>
            <div style={{ fontSize: 12, color: "#9aa0ad" }}>Task {shortId(report.task_id)}</div>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="secondary-button" onClick={() => { if (onRefresh) onRefresh(); }} disabled={loading}>
              {loading ? "Refreshing..." : "Refresh"}
            </button>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
        </div>

        <div className="config-modal-body" style={{ flex: 1, overflowY: "auto", padding: 16 }}>
          {/* Header Info */}
          <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))", gap: 12 }}>
              <div><strong>Original Request</strong> <p style={{ margin: "4px 0 0", color: "#b4b8c6", whiteSpace: "pre-wrap" }}>{report.original_request}</p></div>
              <div><strong>Project</strong> <span style={{ color: "#b4b8c6" }}>{report.project_name || projectName(report.project_id)}</span></div>
              <div><strong>Created</strong> <span style={{ color: "#b4b8c6" }}>{formatTimestamp(report.created_at)}</span></div>
              <div><strong>Updated</strong> <span style={{ color: "#b4b8c6" }}>{formatTimestamp(report.updated_at)}</span></div>
              <div><strong>Elapsed Time</strong> <span style={{ color: "#b4b8c6" }}>{formatDuration(report.elapsed_ms)}</span></div>
              <div><strong>Execution Attempts</strong> <span style={{ color: "#b4b8c6" }}>{report.execution_attempts}</span></div>
              <div><strong>Retries</strong> <span style={{ color: "#b4b8c6" }}>{report.retries}</span></div>
              <div><strong>Corrections</strong> <span style={{ color: "#b4b8c6" }}>{report.corrections}</span></div>
              <div><strong>Files Changed</strong> <span style={{ color: "#b4b8c6" }}>{report.files_changed.created.length + report.files_changed.modified.length + report.files_changed.deleted.length}</span></div>
            </div>
          </div>

          {/* Planning Summary */}
          {report.planning_summary && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 8px", fontSize: 14, color: "#e3e5ec" }}>Planning Summary</h3>
              <p style={{ margin: 0, color: "#b4b8c6", whiteSpace: "pre-wrap" }}>{report.planning_summary}</p>
            </div>
          )}

          {/* Agents & Models */}
          {(report.agents.length > 0 || report.models.length > 0) && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Agents & Models</h3>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
                <div>
                  <strong style={{ color: "#9aa0ad", fontSize: 11 }}>Agents</strong>
                  <ul style={{ margin: "4px 0 0 20", color: "#b4b8c6" }}>
                    {report.agents.map((a) => <li key={a}>{a}</li>)}
                    {report.agents.length === 0 && <li style={{ color: "#9aa0ad" }}>None recorded</li>}
                  </ul>
                </div>
                <div>
                  <strong style={{ color: "#9aa0ad", fontSize: 11 }}>Models</strong>
                  <ul style={{ margin: "4px 0 0 20", color: "#b4b8c6" }}>
                    {report.models.map((m) => <li key={m}>{m}</li>)}
                    {report.models.length === 0 && <li style={{ color: "#9aa0ad" }}>None recorded</li>}
                  </ul>
                </div>
              </div>
            </div>
          )}

          {/* Verification Results */}
          {report.verification_results.length > 0 && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Verification Results ({report.verification_results.length})</h3>
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {report.verification_results.map((v, idx) => (
                  <details key={idx} style={{ background: "#0d1117", border: "1px solid #2f3542", borderRadius: 4, overflow: "hidden" }}>
                    <summary style={{ display: "flex", alignItems: "center", gap: 8, padding: 12, cursor: "pointer" }}>
                      <span style={{ fontSize: 10, padding: "2px 6px", borderRadius: 4, background: `${VERIFICATION_STATUS_COLORS[v.status] || "#9aa0ad"}22`, color: VERIFICATION_STATUS_COLORS[v.status] || "#9aa0ad", fontWeight: 500, textTransform: "uppercase" }}>
                        {v.status.charAt(0).toUpperCase() + v.status.slice(1)}
                      </span>
                      <span style={{ fontFamily: "monospace", color: "#b4b8c6" }}>{v.command}</span>
                      {v.exit_code !== null && <span style={{ fontSize: 11, color: "#9aa0ad" }}>exit {v.exit_code}</span>}
                    </summary>
                    {v.output.trim() ? (
                      <pre style={{ margin: 0, padding: "0 12 12", fontSize: 11, overflow: "auto", maxHeight: 300 }}>{v.output}</pre>
                    ) : (
                      <p style={{ margin: 0, padding: "0 12 12", color: "#9aa0ad" }}>No output captured</p>
                    )}
                  </details>
                ))}
              </div>
            </div>
          )}

          {/* Review Results */}
          {report.review_results.length > 0 && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Review Results ({report.review_results.length})</h3>
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                {report.review_results.map((r, idx) => (
                  <div key={idx} style={{ background: "#0d1117", border: "1px solid #2f3542", borderRadius: 8, padding: 12 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                      <span style={{ fontSize: 11, padding: "2px 8px", borderRadius: 12, background: `${REVIEW_VERDICT_COLORS[r.verdict] || "#9aa0ad"}22`, color: REVIEW_VERDICT_COLORS[r.verdict] || "#9aa0ad", fontWeight: 500, textTransform: "capitalize" }}>
                        {r.verdict.charAt(0).toUpperCase() + r.verdict.slice(1).replace("_", " ")}
                      </span>
                    </div>
                    <p style={{ margin: "0 0 8px", color: "#b4b8c6" }}>{r.findings}</p>
                    {r.unresolved_issues.length > 0 && (
                      <div>
                        <strong style={{ color: "#ff8a8a", fontSize: 12 }}>Unresolved Issues</strong>
                        <ul style={{ margin: "4px 0 0 20", color: "#b4b8c6" }}>
                          {r.unresolved_issues.map((issue, i) => <li key={i}>{issue}</li>)}
                        </ul>
                      </div>
                    )}
                    {r.suggested_fix && (
                      <details style={{ marginTop: 8 }}>
                        <summary style={{ fontSize: 11, color: "#6aa6ff", cursor: "pointer" }}>Suggested Fix</summary>
                        <p style={{ marginTop: 8, color: "#b4b8c6" }}>{r.suggested_fix}</p>
                      </details>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Files Changed */}
          {report.files_changed.created.length > 0 || report.files_changed.modified.length > 0 || report.files_changed.deleted.length > 0 && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Files Changed</h3>
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                {report.files_changed.created.length > 0 && (
                  <div>
                    <strong style={{ color: "#73c69c" }}>Created ({report.files_changed.created.length})</strong>
                    <ul style={{ margin: "4px 0 0 20", color: "#b4b8c6", fontSize: 12, fontFamily: "monospace" }}>
                      {report.files_changed.created.map((f) => <li key={f}>{f}</li>)}
                    </ul>
                  </div>
                )}
                {report.files_changed.modified.length > 0 && (
                  <div>
                    <strong style={{ color: "#f5c842" }}>Modified ({report.files_changed.modified.length})</strong>
                    <ul style={{ margin: "4px 0 0 20", color: "#b4b8c6", fontSize: 12, fontFamily: "monospace" }}>
                      {report.files_changed.modified.map((f) => <li key={f}>{f}</li>)}
                    </ul>
                  </div>
                )}
                {report.files_changed.deleted.length > 0 && (
                  <div>
                    <strong style={{ color: "#ff8a8a" }}>Deleted ({report.files_changed.deleted.length})</strong>
                    <ul style={{ margin: "4px 0 0 20", color: "#b4b8c6", fontSize: 12, fontFamily: "monospace" }}>
                      {report.files_changed.deleted.map((f) => <li key={f}>{f}</li>)}
                    </ul>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Tests & Commands */}
          {report.tests_and_commands.length > 0 && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Tests & Commands Executed</h3>
              <ul style={{ margin: 0, paddingLeft: 20, color: "#b4b8c6" }}>
                {report.tests_and_commands.map((cmd, idx) => (
                  <li key={idx} style={{ fontFamily: "monospace", fontSize: 12, marginBottom: 4 }}>{cmd}</li>
                ))}
              </ul>
            </div>
          )}

          {/* Final Outcome */}
          <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
            <h3 style={{ margin: "0 0 8px", fontSize: 14, color: "#e3e5ec" }}>Final Outcome</h3>
            <p style={{ margin: 0, color: "#b4b8c6", fontWeight: 500 }}>{report.final_outcome}</p>
          </div>

          {/* Remaining Issues */}
          {report.remaining_issues.length > 0 && (
            <div style={{ background: "#2d1111", border: "1px solid #ff8a8a44", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 8px", fontSize: 14, color: "#ff8a8a" }}>Remaining Issues ({report.remaining_issues.length})</h3>
              <ul style={{ margin: 0, paddingLeft: 20, color: "#ff8a8a" }}>
                {report.remaining_issues.map((issue, idx) => <li key={idx}>{issue}</li>)}
              </ul>
            </div>
          )}

          {/* Token Usage */}
          <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
            <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Token Usage</h3>
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

          {/* Known Facts */}
          {report.known_facts.length > 0 && (
            <div style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16, marginBottom: 16 }}>
              <h3 style={{ margin: "0 0 8px", fontSize: 14, color: "#e3e5ec" }}>Known Facts</h3>
              <ul style={{ margin: 0, paddingLeft: 20, color: "#b4b8c6" }}>
                {report.known_facts.map((fact, idx) => <li key={idx}>{fact}</li>)}
              </ul>
            </div>
          )}

          {/* Unavailable Data */}
          {report.unavailable.length > 0 && (
            <div style={{ background: "#2d2510", border: "1px solid #f5c84244", borderRadius: 8, padding: 16 }}>
              <h3 style={{ margin: "0 0 8px", fontSize: 14, color: "#f5c842" }}>Unavailable Data</h3>
              <ul style={{ margin: 0, paddingLeft: 20, color: "#f5c842" }}>
                {report.unavailable.map((item, idx) => <li key={idx}>{item}</li>)}
              </ul>
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