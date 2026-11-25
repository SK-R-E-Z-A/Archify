import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type InstructionScope = "global" | "project" | "agent" | "task";
type InstructionFormat = "markdown" | "plain_text" | "discovered_file";

type InstructionRecord = {
  id: string;
  title: string;
  content: string;
  scope: InstructionScope;
  project_root: string | null;
  project_id: string | null;
  task_id: string | null;
  format: InstructionFormat;
  enabled: boolean;
  agent_targets: string[];
  priority: number;
  tags: string[];
  created_at: string;
  updated_at: string;
  source_path: string | null;
  managed: boolean;
  read_error: string | null;
};

type AppliedInstructionBlock = {
  refs: { id: string; title: string; scope: InstructionScope; source: string }[];
  text: string;
  warnings: string[];
  truncated: boolean;
};

type Props = {
  projectRoot?: string | null;
  projectId?: string | null;
  taskId?: string | null;
  onClose?: () => void;
};

function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  try { return JSON.stringify(e); } catch { return String(e); }
}

const EMPTY_FORM = {
  title: "",
  content: "",
  scope: "global" as InstructionScope,
  enabled: true,
  priority: 0,
  agent_targets: "",
  tags: "",
  format: "markdown" as InstructionFormat,
  task_id: "",
};

export default function InstructionManager({ projectRoot, projectId, taskId, onClose }: Props) {
  const [items, setItems] = useState<InstructionRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [form, setForm] = useState(EMPTY_FORM);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [preview, setPreview] = useState<AppliedInstructionBlock | null>(null);
  const [previewAgent, setPreviewAgent] = useState<"planner" | "executor">(
    "planner",
  );
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  const selected = useMemo(() => items.find((i) => i.id === selectedId) ?? null, [items, selectedId]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<InstructionRecord[]>("list_instructions", {
        projectRoot: projectRoot ?? null,
        projectId: projectId ?? null,
      });
      setItems(list);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, [projectRoot, projectId]);

  useEffect(() => { void refresh(); }, [refresh]);

  function startCreate() {
    setEditingId(null);
    setSelectedId(null);
    setForm({ ...EMPTY_FORM, scope: projectRoot ? "project" : "global", task_id: taskId ?? "" });
    setPreview(null);
    setNotice(null);
  }

  function startEdit(rec: InstructionRecord) {
    if (!rec.managed) { setNotice("Discovered files are read-only references. Create a managed instruction to customize behavior."); return; }
    setEditingId(rec.id);
    setSelectedId(rec.id);
    setForm({
      title: rec.title,
      content: rec.content,
      scope: rec.scope,
      enabled: rec.enabled,
      priority: rec.priority,
      agent_targets: rec.agent_targets.join(", "),
      tags: rec.tags.join(", "),
      format: rec.format === "plain_text" ? "plain_text" : "markdown",
      task_id: rec.task_id ?? "",
    });
    setPreview(null);
  }

  async function save() {
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const payload = {
        id: editingId,
        title: form.title,
        content: form.content,
        scope: form.scope,
        project_root: form.scope === "project" ? (projectRoot ?? null) : null,
        project_id: form.scope === "project" ? (projectId ?? null) : null,
        task_id: form.scope === "task" ? (form.task_id.trim() || taskId || null) : null,
        format: form.format,
        enabled: form.enabled,
        agent_targets: form.agent_targets.split(",").map((s) => s.trim()).filter(Boolean),
        priority: Number(form.priority) || 0,
        tags: form.tags.split(",").map((s) => s.trim()).filter(Boolean),
      };
      const saved = await invoke<InstructionRecord>("upsert_instruction", { input: payload });
      setNotice(editingId ? "Instruction updated." : "Instruction created.");
      setPreview(null);
      setEditingId(saved.id);
      await refresh();
      setSelectedId(saved.id);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setSaving(false);
    }
  }

  async function toggleEnabled(rec: InstructionRecord) {
    if (!rec.managed) return;
    setError(null);
    try {
      await invoke<InstructionRecord>("set_instruction_enabled", { id: rec.id, enabled: !rec.enabled });
      await refresh();
    } catch (e) { setError(errMsg(e)); }
  }

  async function doDelete(id: string) {
    setError(null);
    try {
      await invoke("delete_instruction", { id });
      setConfirmDeleteId(null);
      if (selectedId === id) setSelectedId(null);
      if (editingId === id) { setEditingId(null); setForm(EMPTY_FORM); }
      setPreview(null);
      setNotice("Instruction deleted.");
      await refresh();
    } catch (e) { setError(errMsg(e)); }
  }

  async function runPreview() {
    setError(null);
    try {
      const block = await invoke<AppliedInstructionBlock>("preview_instruction_application", {
        projectRoot: projectRoot ?? null,
        projectId: projectId ?? null,
        agentKind: previewAgent,
      });
      setPreview(block);
    } catch (e) { setError(errMsg(e)); }
  }

  return (
    <div className="instruction-manager" style={{ display: "flex", flexDirection: "column", gap: 12, padding: 12, height: "100%", overflow: "auto", color: "#e5e7eb" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
        <div>
          <strong>Instruction Manager</strong>
          <div style={{ fontSize: 12, color: "#858895" }}>Managed instructions are stored in app data and applied predictably. Existing CLAUDE.md/AGENTS.md files are discovered read-only and never overwritten.</div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button onClick={startCreate}>New</button>
          <button onClick={() => void refresh()} disabled={loading}>{loading ? "Loading…" : "Refresh"}</button>
          <select
            value={previewAgent}
            onChange={(e) => {
              setPreviewAgent(e.target.value as "planner" | "executor");
              setPreview(null);
            }}
            title="Agent stage to preview"
          >
            <option value="planner">planner</option>
            <option value="executor">executor</option>
          </select>
          <button onClick={() => void runPreview()}>Preview Apply</button>
          {onClose && <button onClick={onClose}>Close</button>}
        </div>
      </div>
      {error && <div style={{ color: "#ff8a8a", fontSize: 12 }}>Error: {error}</div>}
      {notice && <div style={{ color: "#8ad1ff", fontSize: 12 }}>{notice}</div>}
      <div style={{ display: "grid", gridTemplateColumns: "320px 1fr", gap: 12, minHeight: 320 }}>
        <div style={{ border: "1px solid #2b2d35", borderRadius: 8, padding: 8, overflow: "auto" }}>
          <div style={{ fontSize: 12, color: "#858895", marginBottom: 8 }}>Instructions ({items.length}) — project: {projectRoot || "none"}</div>
          {items.map((r) => (
            <div key={r.id} style={{ border: "1px solid #333741", borderRadius: 6, padding: 8, marginBottom: 8, background: selectedId === r.id ? "#1d2230" : "#18191e", opacity: r.enabled ? 1 : 0.65 }}>
              <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
                <button onClick={() => { setSelectedId(r.id); setPreview(null); if (!r.managed) setNotice("Discovered files are read-only references. Create a managed instruction to customize behavior."); }} style={{ textAlign: "left", background: "transparent", border: 0, color: "inherit", padding: 0, flex: 1 }}>
                  <div style={{ fontWeight: 600, fontSize: 13 }}>{r.title} {!r.managed ? <span style={{ fontSize: 11, color: "#b8a46a" }}>(discovered)</span> : null}</div>
                  <div style={{ fontSize: 11, color: "#858895" }}>{r.scope} · {r.enabled ? "enabled" : "disabled"} · p{r.priority}{r.agent_targets.length ? ` · ${r.agent_targets.join(",")}` : ""}</div>
                </button>
              </div>
              <div style={{ display: "flex", gap: 6, marginTop: 6, flexWrap: "wrap" }}>
                {r.managed && <button onClick={() => startEdit(r)}>Edit</button>}
                {r.managed && <button onClick={() => void toggleEnabled(r)}>{r.enabled ? "Disable" : "Enable"}</button>}
                {r.managed && <button onClick={() => setConfirmDeleteId(r.id)}>Delete</button>}
              </div>
              {confirmDeleteId === r.id && (
                <div style={{ marginTop: 6, fontSize: 12, color: "#ffcc80" }}>
                  Delete this managed instruction?
                  <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
                    <button onClick={() => void doDelete(r.id)}>Confirm Delete</button>
                    <button onClick={() => setConfirmDeleteId(null)}>Cancel</button>
                  </div>
                </div>
              )}
              {r.read_error && <div style={{ fontSize: 11, color: "#ff8a8a", marginTop: 4 }}>{r.read_error}</div>}
            </div>
          ))}
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ border: "1px solid #2b2d35", borderRadius: 8, padding: 10 }}>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>{editingId ? "Edit Managed Instruction" : "Create Managed Instruction"}</div>
            <div style={{ display: "grid", gap: 8 }}>
              <label>Title<input value={form.title} onChange={(e) => setForm({ ...form, title: e.target.value })} style={{ width: "100%" }} /></label>
              <label>Scope
                <select value={form.scope} onChange={(e) => setForm({ ...form, scope: e.target.value as InstructionScope })}>
                  <option value="global">global</option>
                  <option value="project" disabled={!projectRoot}>project {projectRoot ? "" : "(open project first)"}</option>
                  <option value="agent">agent</option>
                  <option value="task">task</option>
                </select>
              </label>
              <label>Format
                <select value={form.format} onChange={(e) => setForm({ ...form, format: e.target.value as InstructionFormat })}>
                  <option value="markdown">markdown</option>
                  <option value="plain_text">plain_text</option>
                </select>
              </label>
              <label>Priority (lower applies first)<input type="number" value={form.priority} onChange={(e) => setForm({ ...form, priority: Number(e.target.value) })} /></label>
              <label>Agent targets (comma-separated; empty=all)<input value={form.agent_targets} onChange={(e) => setForm({ ...form, agent_targets: e.target.value })} placeholder="planner, executor, reviewer, claude, codex" /></label>
              {form.scope === "task" && (
                <label>Task ID<input value={form.task_id} onChange={(e) => setForm({ ...form, task_id: e.target.value })} placeholder="task id" /></label>
              )}
              <label>Tags<input value={form.tags} onChange={(e) => setForm({ ...form, tags: e.target.value })} /></label>
              <label><input type="checkbox" checked={form.enabled} onChange={(e) => setForm({ ...form, enabled: e.target.checked })} /> Enabled</label>
              <label>Content<textarea value={form.content} onChange={(e) => setForm({ ...form, content: e.target.value })} rows={10} style={{ width: "100%", fontFamily: "monospace" }} /></label>
              <div style={{ display: "flex", gap: 8 }}>
                <button onClick={() => void save()} disabled={saving}>{saving ? "Saving…" : (editingId ? "Save Changes" : "Create")}</button>
                <button onClick={startCreate}>Clear</button>
              </div>
            </div>
          </div>
          <div style={{ border: "1px solid #2b2d35", borderRadius: 8, padding: 10 }}>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>Preview</div>
            {selected && !preview ? (
              <pre style={{ whiteSpace: "pre-wrap", fontSize: 12, margin: 0 }}>{selected.content || "(empty)"}</pre>
            ) : null}
            {preview ? (
              <div>
                <div style={{ fontSize: 12, color: "#858895", marginBottom: 6 }}>Applied refs: {preview.refs.map((r) => r.title).join(", ") || "(none)"}{preview.truncated ? " · truncated" : ""}</div>
                {preview.warnings.map((w, i) => <div key={i} style={{ fontSize: 12, color: "#ffcc80" }}>Warning: {w}</div>)}
                <pre style={{ whiteSpace: "pre-wrap", fontSize: 12, marginTop: 8 }}>{preview.text || "(no managed instructions would be applied)"}</pre>
              </div>
            ) : !selected ? <div style={{ fontSize: 12, color: "#858895" }}>Select an instruction or click Preview Apply.</div> : null}
          </div>
        </div>
      </div>
    </div>
  );
}
