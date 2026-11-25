import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type SearchResult = {
  id: string;
  type: "project" | "task" | "session" | "notification" | "file";
  title: string;
  snippet: string;
  path: string | null;
  project_id: string | null;
};

interface GlobalSearchResultsScreenProps {
  query: string;
}

/**
 * Global Search Results screen — design.md §6.1.
 *
 * Search results across projects, tasks, sessions, notifications, and
 * files. Every button uses the token-driven Button component.
 */
export function GlobalSearchResultsScreen({ query }: GlobalSearchResultsScreenProps) {
  const [results, setResults] = useState<SearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = useCallback(async () => {
    if (!query.trim()) return;
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<SearchResult[]>("search_projects", {
        query: query.trim(),
      });
      setResults(list);
    } catch (e) {
      setError(typeof e === "string" ? e : "Search failed");
    } finally {
      setLoading(false);
    }
  }, [query]);

  useCallback(() => {
    void search();
  }, [search]);

  return (
    <div className="global-search-results-screen" role="main" aria-label="Global Search Results">
      <header className="global-search-results-head">
        <div>
          <h2 className="global-search-results-title">Search Results</h2>
          <p className="global-search-results-sub">
            Results for "{query}"
          </p>
        </div>
        <div className="global-search-results-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void search()}
            disabled={loading}
            title={loading ? "Searching…" : "Re-run search"}
          >
            {loading ? "Searching…" : "Re-run Search"}
          </Button>
        </div>
      </header>

      {error && (
        <div className="global-search-results-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="global-search-results-list">
        {results.length === 0 && !loading && (
          <div className="global-search-results-empty">
            <span className="global-search-results-empty-icon">▱</span>
            <p className="global-search-results-empty-title">No results</p>
            <p className="global-search-results-empty-desc">
              No matches found for "{query}".
            </p>
          </div>
        )}
        {results.length > 0 && (
          <div className="global-search-results-items">
            {results.map((result) => (
              <div
                key={result.id}
                className={`global-search-results-item ${result.type}`}
              >
                <span className="global-search-results-item-type">{result.type}</span>
                <h3 className="global-search-results-item-title">{result.title}</h3>
                <p className="global-search-results-item-snippet">{result.snippet}</p>
                {result.path && (
                  <span className="global-search-results-item-path">{result.path}</span>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default GlobalSearchResultsScreen;
