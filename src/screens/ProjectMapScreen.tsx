import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type FileEntry = {
  name: string;
  path: string;
  is_dir: boolean;
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

interface ProjectMapScreenProps {
  project: ProjectRecord | null;
}

/**
 * Project Map screen — design.md §13.
 *
 * Project-scoped file map showing the directory tree of the active project.
 * Loads directory contents via the list_directory Tauri command and renders
 * an expandable tree view. Every button uses the token-driven Button
 * component and respects theme / density / font-size presets.
 */
export function ProjectMapScreen({ project }: ProjectMapScreenProps) {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loadingPaths, setLoadingPaths] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  const loadDirectory = useCallback(
    async (directoryPath: string | null) => {
      if (!project) return;
      const key = directoryPath ?? project.root;
      setLoadingPaths((prev) => new Set([...prev, key]));
      try {
        const list = await invoke<FileEntry[]>("list_directory", {
          project_root: project.root,
          directory_path: directoryPath ?? undefined,
        });
        setEntries((prev) => {
          const existing = prev.filter(
            (e) =>
              !e.path.startsWith(directoryPath ?? project.root + "/") ||
              e.path === directoryPath,
          );
          return [...existing, ...list];
        });
      } catch (e) {
        setError(typeof e === "string" ? e : "Failed to load directory");
      } finally {
        setLoadingPaths((prev) => {
          const next = new Set(prev);
          next.delete(key);
          return next;
        });
      }
    },
    [project],
  );

  useEffect(() => {
    if (project) {
      void loadDirectory(null);
    }
  }, [project, loadDirectory]);

  const toggleExpand = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const isExpanded = (path: string) => expanded.has(path);

  const isLoading = (path: string) => loadingPaths.has(path);

  const tree = useMemo(() => {
    const byParent = new Map<string, FileEntry[]>();
    const roots: FileEntry[] = [];
    entries.forEach((e) => {
      const parent = e.path.includes("/")
        ? e.path.slice(0, e.path.lastIndexOf("/"))
        : project?.root ?? "";
      if (parent === project?.root) {
        roots.push(e);
      } else {
        const arr = byParent.get(parent) ?? [];
        arr.push(e);
        byParent.set(parent, arr);
      }
    });
    return { roots, byParent };
  }, [entries, project]);

  if (error) {
    return (
      <div className="project-map-screen" role="main" aria-label="Project Map">
        <header className="project-map-head">
          <div>
            <h2 className="project-map-title">{project?.name || "Project Map"}</h2>
            <p className="project-map-sub">File map for this project.</p>
          </div>
        </header>
        <div className="project-map-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); }} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  if (!project) {
    return (
      <div className="project-map-screen" role="main" aria-label="Project Map">
        <header className="project-map-head">
          <div>
            <h2 className="project-map-title">Project Map</h2>
            <p className="project-map-sub">No project selected.</p>
          </div>
        </header>
        <div className="project-map-empty">
          <span className="project-map-empty-icon">▱</span>
          <p className="project-map-empty-title">No project selected</p>
          <p className="project-map-empty-desc">
            Open a project to view its file map.
          </p>
        </div>
      </div>
    );
  }

  const renderTree = (items: FileEntry[], depth = 0) => (
    <div className="project-map-tree-level" style={{ paddingLeft: depth * 16 }}>
      {items.map((entry) => (
        <div
          key={entry.path}
          className={`project-map-tree-item ${entry.is_dir ? "dir" : "file"}`}
        >
          <div
            className="project-map-tree-item-head"
            onClick={() => {
              if (entry.is_dir) {
                toggleExpand(entry.path);
                if (!isExpanded(entry.path) && !isLoading(entry.path)) {
                  void loadDirectory(entry.path);
                }
              } else {
                setSelectedPath(entry.path);
              }
            }}
            role="button"
            tabIndex={0}
          >
            <span className="project-map-tree-item-icon">
              {entry.is_dir
                ? isExpanded(entry.path)
                  ? "▾"
                  : "▸"
                : "📄"}
            </span>
            <span className="project-map-tree-item-name">
              {entry.name}
            </span>
            {entry.is_dir && (
              <Button
                role="ghost"
                size="sm"
                onClick={(e) => {
                  e.stopPropagation();
                  toggleExpand(entry.path);
                  if (!isExpanded(entry.path) && !isLoading(entry.path)) {
                    void loadDirectory(entry.path);
                  }
                }}
                title={isExpanded(entry.path) ? "Collapse" : "Expand"}
              >
                {isExpanded(entry.path) ? "▾" : "▸"}
              </Button>
            )}
          </div>
          {entry.is_dir && isExpanded(entry.path) && (
            <div className="project-map-tree-children">
              {isLoading(entry.path) ? (
                <div className="project-map-tree-loading">Loading…</div>
              ) : (
                renderTree(
                  tree.byParent.get(entry.path) ?? [],
                  depth + 1,
                )
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );

  return (
    <div className="project-map-screen" role="main" aria-label="Project Map">
      <header className="project-map-head">
        <div>
          <h2 className="project-map-title">{project.name}</h2>
          <p className="project-map-sub">File map for this project.</p>
        </div>
        <div className="project-map-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadDirectory(null)}
            disabled={loadingPaths.has(project.root)}
            title={loadingPaths.has(project.root) ? "Loading…" : "Refresh map"}
          >
            {loadingPaths.has(project.root) ? "Loading…" : "Refresh"}
          </Button>
        </div>
      </header>

      <div className="project-map-path">
        <span className="project-map-path-label">Root:</span>
        <span className="project-map-path-value">{project.root}</span>
      </div>

      <div className="project-map-tree-wrap">
        {entries.length === 0 && !loadingPaths.has(project.root) && (
          <div className="project-map-empty">
            <span className="project-map-empty-icon">▱</span>
            <p className="project-map-empty-title">Empty directory</p>
            <p className="project-map-empty-desc">
              The project root contains no files.
            </p>
          </div>
        )}
        {entries.length > 0 && renderTree(tree.roots)}
      </div>

      {selectedPath && (
        <div className="project-map-selected">
          <span className="project-map-selected-label">Selected:</span>
          <span className="project-map-selected-value">{selectedPath}</span>
        </div>
      )}
    </div>
  );
}

export default ProjectMapScreen;
