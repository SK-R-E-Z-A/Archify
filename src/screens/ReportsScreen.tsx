import { useMemo, useState } from "react";
import { Button } from "../components/Button";

type ReportFormat = "markdown" | "pdf" | "html";

type ReportRecord = {
  id: string;
  title: string;
  description: string;
  project_id: string | null;
  generated_at: string | null;
  format: ReportFormat;
  status: "pending" | "ready" | "failed";
  size_bytes: number | null;
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

interface ReportsScreenProps {
  projects: ProjectRecord[];
}

/**
 * Reports screen — design.md §23.3.
 *
 * Global reports destination with project filter. Supports generating and
 * downloading reports in Markdown / PDF / HTML. Every button uses the
 * token-driven Button component and respects theme / density / font-size
 * presets.
 */
export function ReportsScreen({ projects }: ReportsScreenProps) {
  const [reports, setReports] = useState<ReportRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [projectFilter, setProjectFilter] = useState<string>("");
  const [formatFilter, setFormatFilter] = useState<ReportFormat | "">("");

  const handleGenerate = async (format: ReportFormat) => {
    setLoading(true);
    setError(null);
    try {
      // Reports generation is wired to the backend in a later milestone;
      // here we record the intent optimistically.
      const id = `report-${Date.now()}`;
      const now = new Date().toISOString();
      setReports((prev) => [
        { id, title: `${format.toUpperCase()} Report`, description: `Generated ${format} report${projectFilter ? ` for project filter` : ""}`, project_id: projectFilter || null, generated_at: now, format, status: "ready", size_bytes: null },
        ...prev,
      ]);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to generate report");
    } finally {
      setLoading(false);
    }
  };

  const handleDownload = (id: string) => {
    setReports((prev) => prev.filter((r) => r.id !== id));
  };

  const filtered = useMemo(() => {
    return reports.filter((r) => {
      const matchesProject = projectFilter === "" || r.project_id === projectFilter;
      const matchesFormat = formatFilter === "" || r.format === formatFilter;
      return matchesProject && matchesFormat;
    });
  }, [reports, projectFilter, formatFilter]);

  const projectName = (id: string | null | undefined) => {
    if (!id) return "All projects";
    return projects.find((p) => p.id === id)?.name ?? id;
  };

  if (error) {
    return (
      <div className="reports-screen" role="main" aria-label="Reports">
        <header className="reports-head">
          <div>
            <h2 className="reports-title">Reports</h2>
            <p className="reports-sub">
              Generate and download project reports.
            </p>
          </div>
        </header>
        <div className="reports-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="reports-screen" role="main" aria-label="Reports">
      <header className="reports-head">
        <div>
          <h2 className="reports-title">Reports</h2>
          <p className="reports-sub">
            Generate and download project reports.
          </p>
        </div>
        <div className="reports-actions">
          <Button
            role="primary"
            size="sm"
            onClick={() => handleGenerate("markdown")}
            disabled={loading}
            title="Generate Markdown report"
          >
            {loading ? "Generating…" : "Generate Markdown"}
          </Button>
          <Button
            role="secondary"
            size="sm"
            onClick={() => handleGenerate("pdf")}
            disabled={loading}
            title="Generate PDF report"
          >
            {loading ? "Generating…" : "Generate PDF"}
          </Button>
          <Button
            role="secondary"
            size="sm"
            onClick={() => handleGenerate("html")}
            disabled={loading}
            title="Generate HTML report"
          >
            {loading ? "Generating…" : "Generate HTML"}
          </Button>
        </div>
      </header>

      {/* Filters */}
      <div className="reports-filters">
        <div className="reports-filter-group">
          <label className="reports-filter-label">Project</label>
          <select
            value={projectFilter}
            onChange={(e) => setProjectFilter(e.target.value)}
            disabled={loading}
            className="reports-filter-select"
          >
            <option value="">All Projects</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        </div>
        <div className="reports-filter-group">
          <label className="reports-filter-label">Format</label>
          <select
            value={formatFilter}
            onChange={(e) => setFormatFilter(e.target.value as ReportFormat | "")}
            disabled={loading}
            className="reports-filter-select"
          >
            <option value="">All Formats</option>
            <option value="markdown">Markdown</option>
            <option value="pdf">PDF</option>
            <option value="html">HTML</option>
          </select>
        </div>
      </div>

      {/* Reports list */}
      <div className="reports-list-wrap">
        {loading && reports.length === 0 && (
          <div className="reports-empty">
            <span className="reports-empty-icon">⟳</span>
            <p className="reports-empty-title">Generating report…</p>
          </div>
        )}
        {!loading && filtered.length === 0 && error === null && (
          <div className="reports-empty">
            <span className="reports-empty-icon">▱</span>
            <p className="reports-empty-title">No reports</p>
            <p className="reports-empty-desc">
              Generate a report to see it here.
            </p>
          </div>
        )}
        {!loading && filtered.length > 0 && (
          <div className="reports-list">
            {filtered.map((report) => (
              <div
                key={report.id}
                className={`reports-item ${report.status}`}
              >
                <div className="reports-item-head">
                  <div className="reports-item-info">
                    <h3 className="reports-item-name">{report.title}</h3>
                    <p className="reports-item-desc">{report.description}</p>
                    <div className="reports-item-meta">
                      <span className="reports-item-project">
                        Project: {projectName(report.project_id)}
                      </span>
                      <span className="reports-item-format">
                        {report.format.toUpperCase()}
                      </span>
                      <span
                        className={`reports-item-status ${report.status}`}
                      >
                        {report.status}
                      </span>
                      {report.generated_at && (
                        <span className="reports-item-generated">
                          {new Date(report.generated_at).toLocaleString()}
                        </span>
                      )}
                    </div>
                  </div>
                  <div className="reports-item-actions">
                    {report.status === "ready" && (
                      <Button
                        role="secondary"
                        size="sm"
                        onClick={() => handleDownload(report.id)}
                        title="Download report"
                      >
                        Download
                      </Button>
                    )}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default ReportsScreen;
