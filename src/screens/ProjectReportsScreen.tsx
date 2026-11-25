import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

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

type ProjectReport = {
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

interface ProjectReportsScreenProps {
  project: ProjectRecord | null;
}

/**
 * Project Reports screen — design.md §23.4.
 *
 * Project-scoped report showing task statistics, completion rate,
 * verification results, and token usage for the active project.
 * Every button uses the token-driven Button component and respects
 * theme / density / font-size presets.
 */
export function ProjectReportsScreen({ project }: ProjectReportsScreenProps) {
  const [report, setReport] = useState<ProjectReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadReport = useCallback(async () => {
    if (!project) return;
    setLoading(true);
    setError(null);
    try {
      const r = await invoke<ProjectReport>("get_project_report", {
        projectId: project.id,
      });
      setReport(r);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load project report");
    } finally {
      setLoading(false);
    }
  }, [project]);

  useCallback(() => {
    void loadReport();
  }, [loadReport]);

  if (error) {
    return (
      <div className="project-reports-screen" role="main" aria-label="Project Reports">
        <header className="project-reports-head">
          <div>
            <h2 className="project-reports-title">{project?.name || "Project Reports"}</h2>
            <p className="project-reports-sub">Project statistics and report.</p>
          </div>
        </header>
        <div className="project-reports-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void loadReport(); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  if (!report) {
    return (
      <div className="project-reports-screen" role="main" aria-label="Project Reports">
        <header className="project-reports-head">
          <div>
            <h2 className="project-reports-title">{project?.name || "Project Reports"}</h2>
            <p className="project-reports-sub">Project statistics and report.</p>
          </div>
          <div className="project-reports-actions">
            <Button
              role="primary"
              size="sm"
              onClick={() => void loadReport()}
              disabled={loading}
              title={loading ? "Generating…" : "Generate report"}
            >
              {loading ? "Generating…" : "Generate Report"}
            </Button>
          </div>
        </header>
        <div className="project-reports-empty">
          <span className="project-reports-empty-icon">▱</span>
          <p className="project-reports-empty-title">No report yet</p>
          <p className="project-reports-empty-desc">
            Generate a report to see project statistics.
          </p>
        </div>
      </div>
    );
  }

  const completionRate = Math.round(report.completion_rate * 100);

  return (
    <div className="project-reports-screen" role="main" aria-label="Project Reports">
      <header className="project-reports-head">
        <div>
          <h2 className="project-reports-title">{report.name}</h2>
          <p className="project-reports-sub">Project statistics and report.</p>
        </div>
        <div className="project-reports-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadReport()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh report"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {/* Key metrics */}
      <div className="project-reports-metrics">
        <div className="project-reports-metric">
          <span className="project-reports-metric-value">{report.total_tasks}</span>
          <span className="project-reports-metric-label">Total Tasks</span>
        </div>
        <div className="project-reports-metric">
          <span className="project-reports-metric-value">{report.active_tasks}</span>
          <span className="project-reports-metric-label">Active</span>
        </div>
        <div className="project-reports-metric">
          <span className="project-reports-metric-value">{report.completed_tasks}</span>
          <span className="project-reports-metric-label">Completed</span>
        </div>
        <div className="project-reports-metric">
          <span className="project-reports-metric-value">{report.failed_tasks}</span>
          <span className="project-reports-metric-label">Failed</span>
        </div>
        <div className="project-reports-metric">
          <span className="project-reports-metric-value">{completionRate}%</span>
          <span className="project-reports-metric-label">Completion Rate</span>
        </div>
      </div>

      {/* Details grid */}
      <div className="project-reports-details">
        <div className="project-reports-detail-card">
          <h3 className="project-reports-detail-title">Verification</h3>
          <div className="project-reports-detail-row">
            <span className="project-reports-detail-label">Passed</span>
            <span className="project-reports-detail-value success">{report.verification_passed}</span>
          </div>
          <div className="project-reports-detail-row">
            <span className="project-reports-detail-label">Failed</span>
            <span className="project-reports-detail-value failure">{report.verification_failed}</span>
          </div>
        </div>
        <div className="project-reports-detail-card">
          <h3 className="project-reports-detail-title">Execution</h3>
          <div className="project-reports-detail-row">
            <span className="project-reports-detail-label">Attempts</span>
            <span className="project-reports-detail-value">{report.execution_attempts}</span>
          </div>
          <div className="project-reports-detail-row">
            <span className="project-reports-detail-label">Retries</span>
            <span className="project-reports-detail-value">{report.retries}</span>
          </div>
        </div>
        <div className="project-reports-detail-card">
          <h3 className="project-reports-detail-title">Tokens</h3>
          <div className="project-reports-detail-row">
            <span className="project-reports-detail-label">Input</span>
            <span className="project-reports-detail-value">{report.token_input ?? 0}</span>
          </div>
          <div className="project-reports-detail-row">
            <span className="project-reports-detail-label">Output</span>
            <span className="project-reports-detail-value">{report.token_output ?? 0}</span>
          </div>
          <div className="project-reports-detail-row">
            <span className="project-reports-detail-label">Total</span>
            <span className="project-reports-detail-value">{report.token_total ?? 0}</span>
          </div>
        </div>
      </div>

      {/* Recent failures */}
      {report.recent_failures.length > 0 && (
        <div className="project-reports-section">
          <h3 className="project-reports-section-title">Recent Failures</h3>
          <ul className="project-reports-list">
            {report.recent_failures.map((failure, i) => (
              <li key={i} className="project-reports-list-item">{failure}</li>
            ))}
          </ul>
        </div>
      )}

      {/* Recent tasks */}
      {report.recent_tasks.length > 0 && (
        <div className="project-reports-section">
          <h3 className="project-reports-section-title">Recent Tasks</h3>
          <ul className="project-reports-list">
            {report.recent_tasks.map((task, i) => (
              <li key={i} className="project-reports-list-item">{task}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

export default ProjectReportsScreen;
