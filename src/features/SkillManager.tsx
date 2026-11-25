import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type SkillSource = "managed" | "imported" | "discovered";

type SkillValidationIssue = {
  skill_id: string | null;
  level: string;
  message: string;
};

type SkillRecord = {
  id: string;
  name: string;
  description: string;
  category: string;
  supported_agents: string[];
  tags: string[];
  enabled: boolean;
  source: SkillSource;
  managed: boolean;
  content: string;
  raw_frontmatter: string;
  source_path: string | null;
  directory: string | null;
  created_at: string;
  updated_at: string;
  read_error: string | null;
  validation_issues: SkillValidationIssue[];
};

type AppliedSkillBlock = {
  refs: { id: string; name: string; category: string; source: string }[];
  text: string;
  warnings: string[];
  truncated: boolean;
};

type Props = {
  projectRoot?: string | null;
  onClose?: () => void;
};

type SkillForm = {
  name: string;
  description: string;
  category: string;
  supported_agents: string;
  tags: string;
  enabled: boolean;
  content: string;
};

const EMPTY_FORM: SkillForm = {
  name: "",
  description: "",
  category: "",
  supported_agents: "",
  tags: "",
  enabled: true,
  content: "",
};

const AGENT_OPTIONS = [
  "planner",
  "executor",
  "reviewer",
  "claude",
  "codex",
  "aider",
  "gemini",
  "opencode",
  "cursor-agent",
  "custom",
];

function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

function splitList(value: string): string[] {
  return value
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

function issueCount(record: SkillRecord, level: string): number {
  return record.validation_issues.filter((i) => i.level === level).length;
}

export default function SkillManager({ projectRoot, onClose }: Props) {
  const [items, setItems] = useState<SkillRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [form, setForm] = useState<SkillForm>(EMPTY_FORM);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [importPath, setImportPath] = useState("");
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [selectedForPreview, setSelectedForPreview] = useState<string[]>([]);
  const [previewAgent, setPreviewAgent] = useState("planner");
  const [preview, setPreview] = useState<AppliedSkillBlock | null>(null);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<"all" | "enabled" | "disabled">("all");

  const selected = useMemo(
    () => items.find((i) => i.id === selectedId) ?? null,
    [items, selectedId],
  );

  const visibleItems = useMemo(() => {
    const term = search.trim().toLowerCase();
    return items.filter((item) => {
      if (filter === "enabled" && !item.enabled) return false;
      if (filter === "disabled" && item.enabled) return false;
      if (!term) return true;
      return (
        item.name.toLowerCase().includes(term) ||
        item.description.toLowerCase().includes(term) ||
        item.category.toLowerCase().includes(term) ||
        item.tags.some((tag) => tag.includes(term))
      );
    });
  }, [items, search, filter]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<SkillRecord[]>("list_skills", {
        projectRoot: projectRoot ?? null,
      });
      setItems(list);
      setSelectedForPreview((current) =>
        current.filter((id) => list.some((item) => item.id === id && item.managed)),
      );
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, [projectRoot]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  function startCreate() {
    setEditingId(null);
    setSelectedId(null);
    setForm(EMPTY_FORM);
    setPreview(null);
    setNotice(null);
    setError(null);
  }

  function startEdit(record: SkillRecord) {
    if (!record.managed) {
      setNotice(
        "Discovered skills are read-only references. Import one to edit and enable it in Archify.",
      );
      return;
    }
    setEditingId(record.id);
    setSelectedId(record.id);
    setForm({
      name: record.name,
      description: record.description,
      category: record.category,
      supported_agents: record.supported_agents.join(", "),
      tags: record.tags.join(", "),
      enabled: record.enabled,
      content: record.content,
    });
    setPreview(null);
    setNotice(null);
    setError(null);
  }

  async function save() {
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const payload = {
        id: editingId,
        name: form.name,
        description: form.description,
        category: form.category,
        supported_agents: splitList(form.supported_agents),
        tags: splitList(form.tags),
        enabled: form.enabled,
        content: form.content,
      };
      const saved = await invoke<SkillRecord>("upsert_skill", { input: payload });
      setNotice(editingId ? "Skill updated and saved to the library." : "Skill created.");
      setEditingId(saved.id);
      setSelectedId(saved.id);
      setForm({
        name: saved.name,
        description: saved.description,
        category: saved.category,
        supported_agents: saved.supported_agents.join(", "),
        tags: saved.tags.join(", "),
        enabled: saved.enabled,
        content: saved.content,
      });
      await refresh();
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setSaving(false);
    }
  }

  async function toggleEnabled(record: SkillRecord) {
    if (!record.managed) return;
    setError(null);
    try {
      await invoke<SkillRecord>("set_skill_enabled", {
        id: record.id,
        enabled: !record.enabled,
      });
      await refresh();
    } catch (e) {
      setError(errMsg(e));
    }
  }

  async function doDelete(id: string) {
    setError(null);
    try {
      await invoke("delete_skill", { id });
      setConfirmDeleteId(null);
      setSelectedForPreview((current) => current.filter((item) => item !== id));
      if (selectedId === id) setSelectedId(null);
      if (editingId === id) {
        setEditingId(null);
        setForm(EMPTY_FORM);
      }
      setNotice("Skill deleted.");
      await refresh();
    } catch (e) {
      setError(errMsg(e));
    }
  }

  async function doImport(path?: string) {
    const sourcePath = (path ?? importPath).trim();
    if (!sourcePath) {
      setError("Enter a path to a SKILL.md file or a skill directory to import.");
      return;
    }
    setImporting(true);
    setError(null);
    setNotice(null);
    try {
      const imported = await invoke<SkillRecord>("import_skill", { sourcePath });
      setImportPath("");
      setNotice(`Imported '${imported.name}' into the skill library.`);
      setSelectedId(imported.id);
      setEditingId(null);
      await refresh();
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setImporting(false);
    }
  }

  async function runPreview() {
    setError(null);
    try {
      const block = await invoke<AppliedSkillBlock>("preview_skill_application", {
        skillIds: selectedForPreview,
        agentKind: previewAgent,
      });
      setPreview(block);
    } catch (e) {
      setError(errMsg(e));
    }
  }

  function togglePreviewSelection(id: string) {
    setSelectedForPreview((current) =>
      current.includes(id) ? current.filter((item) => item !== id) : [...current, id],
    );
  }

  const activePreviewCount = selectedForPreview.length;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 12,
        padding: 12,
        height: "100%",
        overflow: "auto",
        color: "#e5e7eb",
      }}
    >
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
        <div>
          <strong>Skills Manager</strong>
          <div style={{ fontSize: 12, color: "#858895" }}>
            Skills are reusable, task-specific procedures stored as SKILL.md files (Agent Skills
            format). They are separate from instructions and are never applied unless explicitly
            selected for a task.
          </div>
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}>
          <button onClick={startCreate}>New Skill</button>
          <button onClick={() => void refresh()} disabled={loading}>
            {loading ? "Loading…" : "Refresh"}
          </button>
          {onClose && <button onClick={onClose}>Close</button>}
        </div>
      </div>

      {error && <div style={{ color: "#ff8a8a", fontSize: 12 }}>Error: {error}</div>}
      {notice && <div style={{ color: "#8ad1ff", fontSize: 12 }}>{notice}</div>}

      <div
        style={{
          border: "1px solid #2b2d35",
          borderRadius: 8,
          padding: 10,
          display: "flex",
          gap: 8,
          flexWrap: "wrap",
          alignItems: "center",
        }}
      >
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search name, description, category, tag"
          style={{ flex: "1 1 240px" }}
        />
        <select value={filter} onChange={(e) => setFilter(e.target.value as typeof filter)}>
          <option value="all">all skills</option>
          <option value="enabled">enabled only</option>
          <option value="disabled">disabled only</option>
        </select>
        <span style={{ fontSize: 12, color: "#858895" }}>
          {visibleItems.length} of {items.length}
          {projectRoot ? " · includes project discoveries" : " · open a project to discover skills"}
        </span>
      </div>

      <div
        style={{
          border: "1px solid #2b2d35",
          borderRadius: 8,
          padding: 10,
          display: "flex",
          gap: 8,
          flexWrap: "wrap",
          alignItems: "center",
        }}
      >
        <span style={{ fontSize: 12, color: "#858895" }}>Import existing skill</span>
        <input
          value={importPath}
          onChange={(e) => setImportPath(e.target.value)}
          placeholder="Path to a SKILL.md or skill directory"
          style={{ flex: "1 1 320px" }}
        />
        <button onClick={() => void doImport()} disabled={importing}>
          {importing ? "Importing…" : "Import"}
        </button>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "340px 1fr", gap: 12, minHeight: 360 }}>
        <div style={{ border: "1px solid #2b2d35", borderRadius: 8, padding: 8, overflow: "auto", maxHeight: "60vh" }}>
          {loading && items.length === 0 ? (
            <div style={{ fontSize: 12, color: "#858895" }}>Loading skills…</div>
          ) : null}
          {!loading && visibleItems.length === 0 ? (
            <div style={{ fontSize: 12, color: "#858895", lineHeight: 1.5 }}>
              {items.length === 0
                ? "No skills yet. Create a procedure or import an existing SKILL.md."
                : "No skills match this search/filter."}
              {projectRoot ? (
                <div style={{ marginTop: 6 }}>
                  Project skills in <code>.agents/skills</code> and <code>.claude/skills</code> appear
                  here as read-only discoveries.
                </div>
              ) : null}
            </div>
          ) : null}
          {visibleItems.map((record) => {
            const errors = issueCount(record, "error");
            const warnings = issueCount(record, "warning");
            return (
              <div
                key={record.id}
                style={{
                  border: "1px solid #333741",
                  borderRadius: 6,
                  padding: 8,
                  marginBottom: 8,
                  background: selectedId === record.id ? "#1d2230" : "#18191e",
                  opacity: record.enabled ? 1 : 0.7,
                }}
              >
                <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
                  <button
                    onClick={() => {
                      setSelectedId(record.id);
                      setPreview(null);
                    }}
                    style={{
                      textAlign: "left",
                      background: "transparent",
                      border: 0,
                      color: "inherit",
                      padding: 0,
                      flex: 1,
                    }}
                  >
                    <div style={{ fontWeight: 600, fontSize: 13 }}>
                      {record.name}{" "}
                      <span
                        style={{
                          fontSize: 10,
                          color:
                            record.source === "discovered"
                              ? "#b8a46a"
                              : record.source === "imported"
                                ? "#8ad1ff"
                                : "#9be28a",
                        }}
                      >
                        {record.source}
                      </span>
                    </div>
                    <div style={{ fontSize: 11, color: "#858895" }}>
                      {record.category || "uncategorized"} ·{" "}
                      {record.enabled ? "enabled" : "disabled"}
                      {record.supported_agents.length
                        ? ` · ${record.supported_agents.join(", ")}`
                        : " · all agents"}
                    </div>
                    {record.description ? (
                      <div style={{ fontSize: 11, color: "#a9adba", marginTop: 2 }}>
                        {record.description}
                      </div>
                    ) : null}
                    {errors || warnings ? (
                      <div style={{ fontSize: 11, marginTop: 3 }}>
                        {errors ? <span style={{ color: "#ff8a8a" }}>{errors} error(s) </span> : null}
                        {warnings ? (
                          <span style={{ color: "#ffcc80" }}>{warnings} warning(s)</span>
                        ) : null}
                      </div>
                    ) : null}
                  </button>
                  {record.managed ? (
                    <input
                      type="checkbox"
                      title="Select for application preview"
                      checked={selectedForPreview.includes(record.id)}
                      onChange={() => togglePreviewSelection(record.id)}
                    />
                  ) : null}
                </div>
                <div style={{ display: "flex", gap: 6, marginTop: 6, flexWrap: "wrap" }}>
                  <button onClick={() => startEdit(record)}>
                    {record.managed ? "Edit" : "Import to edit"}
                  </button>
                  {record.managed ? (
                    <button onClick={() => void toggleEnabled(record)}>
                      {record.enabled ? "Disable" : "Enable"}
                    </button>
                  ) : null}
                  {record.managed ? (
                    <button onClick={() => setConfirmDeleteId(record.id)}>Delete</button>
                  ) : null}
                  {!record.managed && record.source_path ? (
                    <button
                      onClick={() =>
                        void doImport(record.directory ?? record.source_path ?? "")
                      }
                      disabled={importing}
                    >
                      Import
                    </button>
                  ) : null}
                </div>
                {confirmDeleteId === record.id && (
                  <div style={{ marginTop: 6, fontSize: 12, color: "#ffcc80" }}>
                    Delete this skill and its managed SKILL.md directory?
                    <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
                      <button onClick={() => void doDelete(record.id)}>Confirm Delete</button>
                      <button onClick={() => setConfirmDeleteId(null)}>Cancel</button>
                    </div>
                  </div>
                )}
                {record.read_error && (
                  <div style={{ fontSize: 11, color: "#ff8a8a", marginTop: 4 }}>
                    {record.read_error}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ border: "1px solid #2b2d35", borderRadius: 8, padding: 10 }}>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>
              {editingId ? "Edit Skill" : "Create Skill"}
            </div>
            <div style={{ display: "grid", gap: 8 }}>
              <label>
                Name (lowercase letters, numbers, -, _, .)
                <input
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                  placeholder="e.g. tdd"
                  style={{ width: "100%" }}
                />
              </label>
              <label>
                Description (when should an agent use this?)
                <textarea
                  value={form.description}
                  onChange={(e) => setForm({ ...form, description: e.target.value })}
                  rows={2}
                  style={{ width: "100%" }}
                />
              </label>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                <label>
                  Category
                  <input
                    value={form.category}
                    onChange={(e) => setForm({ ...form, category: e.target.value })}
                    placeholder="engineering"
                    style={{ width: "100%" }}
                  />
                </label>
                <label>
                  Tags (comma-separated)
                  <input
                    value={form.tags}
                    onChange={(e) => setForm({ ...form, tags: e.target.value })}
                    placeholder="testing, workflow"
                    style={{ width: "100%" }}
                  />
                </label>
              </div>
              <label>
                Supported agents (comma-separated; empty = all)
                <input
                  value={form.supported_agents}
                  onChange={(e) => setForm({ ...form, supported_agents: e.target.value })}
                  placeholder={AGENT_OPTIONS.join(", ")}
                  style={{ width: "100%" }}
                />
              </label>
              <label>
                <input
                  type="checkbox"
                  checked={form.enabled}
                  onChange={(e) => setForm({ ...form, enabled: e.target.checked })}
                />{" "}
                Enabled
              </label>
              <label>
                Procedure / Markdown body
                <textarea
                  value={form.content}
                  onChange={(e) => setForm({ ...form, content: e.target.value })}
                  rows={12}
                  style={{ width: "100%", fontFamily: "monospace" }}
                  placeholder={"# Procedure\n\n1. Inspect the code.\n2. Make the change.\n3. Verify."}
                />
              </label>
              <div style={{ display: "flex", gap: 8 }}>
                <button onClick={() => void save()} disabled={saving}>
                  {saving ? "Saving…" : editingId ? "Save Changes" : "Create"}
                </button>
                <button
                  onClick={() => {
                    setEditingId(null);
                    setForm(EMPTY_FORM);
                  }}
                >
                  Clear
                </button>
              </div>
            </div>
          </div>

          <div style={{ border: "1px solid #2b2d35", borderRadius: 8, padding: 10 }}>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>Skill Preview</div>
            {selected ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                  <strong style={{ fontSize: 13 }}>{selected.name}</strong>
                  <span style={{ fontSize: 11, color: "#858895" }}>
                    {selected.source} · {selected.category || "uncategorized"}
                  </span>
                  {selected.managed ? <button onClick={() => startEdit(selected)}>Edit</button> : null}
                  {!selected.managed && selected.source_path ? (
                    <button
                      onClick={() =>
                        void doImport(selected.directory ?? selected.source_path ?? "")
                      }
                    >
                      Import into library
                    </button>
                  ) : null}
                </div>
                <div style={{ fontSize: 12, color: "#a9adba" }}>{selected.description || "(no description)"}</div>
                <div style={{ fontSize: 11, color: "#858895" }}>
                  agents: {selected.supported_agents.join(", ") || "all"}
                  {selected.tags.length ? ` · tags: ${selected.tags.join(", ")}` : ""}
                </div>
                {selected.directory ? (
                  <div style={{ fontSize: 11, color: "#858895", wordBreak: "break-all" }}>
                    directory: {selected.directory}
                  </div>
                ) : null}
                {selected.validation_issues.length ? (
                  <div>
                    {selected.validation_issues.map((issue, index) => (
                      <div
                        key={`${issue.level}-${index}`}
                        style={{
                          fontSize: 12,
                          color: issue.level === "error" ? "#ff8a8a" : "#ffcc80",
                        }}
                      >
                        {issue.level}: {issue.message}
                      </div>
                    ))}
                  </div>
                ) : null}
                <pre
                  style={{
                    whiteSpace: "pre-wrap",
                    fontSize: 12,
                    margin: 0,
                    borderTop: "1px solid #2b2d35",
                    paddingTop: 8,
                    maxHeight: "30vh",
                    overflow: "auto",
                  }}
                >
                  {selected.content || "(empty procedure)"}
                </pre>
              </div>
            ) : (
              <div style={{ fontSize: 12, color: "#858895" }}>
                Select a skill to preview its procedure, or edit the form above.
              </div>
            )}
          </div>

          <div style={{ border: "1px solid #2b2d35", borderRadius: 8, padding: 10 }}>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>Application Preview (not applied)</div>
            <div style={{ fontSize: 12, color: "#858895", marginBottom: 8 }}>
              Skills are selected per task in a future integration. This preview shows exactly what
              would be passed to an agent; it does not modify prompts or start a task.
            </div>
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <select value={previewAgent} onChange={(e) => setPreviewAgent(e.target.value)}>
                {AGENT_OPTIONS.map((agent) => (
                  <option key={agent} value={agent}>
                    {agent}
                  </option>
                ))}
              </select>
              <button onClick={() => void runPreview()} disabled={activePreviewCount === 0}>
                Preview Selected ({activePreviewCount})
              </button>
              {activePreviewCount > 0 ? (
                <button onClick={() => setSelectedForPreview([])}>Clear Selection</button>
              ) : null}
            </div>
            {activePreviewCount === 0 ? (
              <div style={{ fontSize: 12, color: "#858895", marginTop: 8 }}>
                Select one or more managed skills using the checkboxes in the list.
              </div>
            ) : null}
            {preview ? (
              <div style={{ marginTop: 8 }}>
                <div style={{ fontSize: 12, color: "#858895", marginBottom: 6 }}>
                  Applied refs: {preview.refs.map((ref) => ref.name).join(", ") || "(none)"}
                  {preview.truncated ? " · truncated" : ""}
                </div>
                {preview.warnings.map((warning, index) => (
                  <div key={index} style={{ fontSize: 12, color: "#ffcc80" }}>
                    Warning: {warning}
                  </div>
                ))}
                <pre style={{ whiteSpace: "pre-wrap", fontSize: 12, marginTop: 6 }}>
                  {preview.text || "(no skills would be applied)"}
                </pre>
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
