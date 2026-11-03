import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, test, vi } from "vitest";
import {
  draftKey,
  loadDraft,
  loadOptions,
  optionsKey,
  resolveWorkspace,
  useDraft,
} from "../src/draft";
import { initialDraft } from "../src/model";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const workspace = "browser:test-workspace";

afterEach(() => {
  localStorage.clear();
  document.querySelector('meta[name="soc-workspace"]')?.remove();
  delete document.documentElement.dataset.workspace;
  vi.useRealTimers();
});

describe("Eventy draft storage", () => {
  test("migrates the legacy draft into an exact workspace-scoped v2 envelope", () => {
    setWorkspace();
    localStorage.setItem("soc-minis-event-v1", JSON.stringify({ club: "UQ Robotics", indoor: false, attendance: 40 }));

    const draft = loadDraft();
    const envelope = JSON.parse(localStorage.getItem(draftKey(workspace)) || "null");

    expect(draft.club).toBe("UQ Robotics");
    expect(draft.indoor).toBe(false);
    expect(draft.attendance).toBe("20");
    expect(localStorage.getItem("soc-minis-event-v1")).toBeNull();
    expect(Object.keys(envelope)).toEqual(["version", "workspace", "mini", "form", "schema", "savedAt", "values"]);
    expect(envelope).toEqual({
      version: 2,
      workspace,
      mini: "eventy",
      form: "prepare",
      schema: "event-draft@2",
      savedAt: expect.any(String),
      values: { ...initialDraft, club: "UQ Robotics", indoor: false },
    });
  });

  test("debounces edits for 250 ms before writing the current workspace", async () => {
    vi.useFakeTimers();
    setWorkspace();
    const host = document.createElement("div"),
      root = createRoot(host);
    document.body.append(host);

    await act(async () => root.render(<DraftHarness />));
    await act(async () => host.querySelector<HTMLButtonElement>('[data-action="edit"]')?.click());
    await act(async () => vi.advanceTimersByTime(249));
    expect(localStorage.getItem(draftKey(workspace))).toBeNull();

    await act(async () => vi.advanceTimersByTime(1));
    expect(JSON.parse(localStorage.getItem(draftKey(workspace)) || "null").values.club).toBe("UQ Robotics");

    await act(async () => root.unmount());
    host.remove();
  });

  test("uses a valid hosted workspace before browser storage", () => {
    setWorkspace();
    const meta = document.createElement("meta");
    meta.name = "soc-workspace";
    meta.content = "society-8291";
    document.head.append(meta);
    document.documentElement.dataset.workspace = "ignored-workspace";

    expect(resolveWorkspace()).toBe("hosted:society-8291");
  });

  test("ignores corrupt and oversized current payloads", () => {
    setWorkspace();
    localStorage.setItem(draftKey(workspace), "{broken");
    expect(loadDraft()).toEqual(initialDraft);

    localStorage.setItem(draftKey(workspace), `{"padding":"${"x".repeat(128 * 1024)}"}`);
    expect(loadDraft()).toEqual(initialDraft);
  });

  test("migrates output choices into the workspace envelope", () => {
    setWorkspace();
    localStorage.setItem("soc-minis-event-options-v1", JSON.stringify({ mode: "risk", layout: "pack" }));

    expect(loadOptions()).toEqual({ mode: "risk", layout: "pack" });
    expect(JSON.parse(localStorage.getItem(optionsKey(workspace)) || "null")).toMatchObject({
      version: 2,
      workspace,
      mini: "eventy",
      form: "options",
      schema: "event-options@2",
      values: { mode: "risk", layout: "pack" },
    });
    expect(localStorage.getItem("soc-minis-event-options-v1")).toBeNull();
  });

  test("clears the saved draft without recreating it during unmount", async () => {
    vi.useFakeTimers();
    setWorkspace();
    const host = document.createElement("div"),
      root = createRoot(host);
    document.body.append(host);

    await act(async () => root.render(<DraftHarness />));
    await act(async () => host.querySelector<HTMLButtonElement>('[data-action="edit"]')?.click());
    await act(async () => vi.advanceTimersByTime(250));
    expect(localStorage.getItem(draftKey(workspace))).not.toBeNull();

    await act(async () => host.querySelector<HTMLButtonElement>('[data-action="clear"]')?.click());
    expect(host.querySelector('[data-value="club"]')?.textContent).toBe("");
    expect(localStorage.getItem(draftKey(workspace))).toBeNull();
    await act(async () => root.unmount());
    expect(localStorage.getItem(draftKey(workspace))).toBeNull();
    host.remove();
  });
});

function setWorkspace() {
  localStorage.setItem("soc-minis:workspace:v1", workspace);
}

function DraftHarness() {
  const [draft, setDraft, clear] = useDraft();
  return (
    <div>
      <span data-value="club">{draft.club}</span>
      <button data-action="edit" type="button" onClick={() => setDraft((current) => ({ ...current, club: "UQ Robotics" }))}>Edit</button>
      <button data-action="clear" type="button" onClick={clear}>Clear</button>
    </div>
  );
}
