import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { initialDraft, type EventDraft } from "./model";

const legacyDraft = "soc-minis-event-v1";
const legacyOptions = "soc-minis-event-options-v1";
const workspaceStorage = "soc-minis:workspace:v1";
const schema = "event-draft@2";
const optionSchema = "event-options@2";
const limit = 128 * 1024;
const delay = 250;
const stringFields = [
  "club", "title", "date", "summary", "attendance", "campus", "building", "room", "roomSeats",
  "coordinator", "role", "email", "phone", "setupStart", "setupEnd", "eventStart", "eventEnd",
  "packStart", "packEnd", "food", "affiliated", "flexibility", "under18", "nonUq",
] as const satisfies readonly (keyof EventDraft)[];
const booleanFields = [
  "indoor", "seated", "vehicles", "structures", "electrical", "cash", "alcohol", "competition", "offSite",
] as const satisfies readonly (keyof EventDraft)[];

export type DraftEnvelope = {
  version: 2;
  workspace: string;
  mini: "eventy";
  form: "prepare";
  schema: typeof schema;
  savedAt: string;
  values: EventDraft;
};

type Options = { mode: string; layout: string };

export function resolveWorkspace(storage: Storage = localStorage) {
  const supplied = document.querySelector<HTMLMetaElement>('meta[name="soc-workspace"]')?.content
    || document.documentElement.dataset.workspace;
  if (supplied && /^[A-Za-z0-9_-]{8,128}$/.test(supplied)) return `hosted:${supplied}`;
  const saved = storage.getItem(workspaceStorage);
  if (saved && /^browser:[A-Za-z0-9-]{8,128}$/.test(saved)) return saved;
  const id = typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  const workspace = `browser:${id}`;
  storage.setItem(workspaceStorage, workspace);
  return workspace;
}

export function draftKey(workspace: string) {
  return `soc-minis:${workspace}:eventy:prepare:v2`;
}

export function optionsKey(workspace: string) {
  return `soc-minis:${workspace}:eventy:options:v2`;
}

export function loadDraft(storage: Storage = localStorage, workspace = resolveWorkspace(storage)): EventDraft {
  const current = parse(storage.getItem(draftKey(workspace)));
  if (isEnvelope(current, workspace)) return normalise(current.values);

  const legacy = parse(storage.getItem(legacyDraft));
  if (!record(legacy)) return { ...initialDraft };
  const migrated = normalise(legacy);
  saveDraft(storage, migrated, workspace);
  storage.removeItem(legacyDraft);
  return migrated;
}

export function saveDraft(storage: Storage, draft: EventDraft, workspace: string) {
  const value: DraftEnvelope = {
    version: 2,
    workspace,
    mini: "eventy",
    form: "prepare",
    schema,
    savedAt: new Date().toISOString(),
    values: normalise(draft),
  };
  const encoded = JSON.stringify(value);
  if (bytes(encoded) > limit) return false;
  storage.setItem(draftKey(workspace), encoded);
  return true;
}

export function loadOptions(storage: Storage = localStorage, workspace = resolveWorkspace(storage)): Options {
  const current = parse(storage.getItem(optionsKey(workspace)));
  if (isOptions(current, workspace)) return normaliseOptions(current.values);
  const legacy = parse(storage.getItem(legacyOptions));
  const options = normaliseOptions(legacy);
  if (record(legacy)) {
    saveOptions(storage, options, workspace);
    storage.removeItem(legacyOptions);
  }
  return options;
}

export function saveOptions(storage: Storage, values: Options, workspace: string) {
  storage.setItem(optionsKey(workspace), JSON.stringify({
    version: 2,
    workspace,
    mini: "eventy",
    form: "options",
    schema: optionSchema,
    savedAt: new Date().toISOString(),
    values: normaliseOptions(values),
  }));
}

export function useDraft(supplied?: string): [EventDraft, Dispatch<SetStateAction<EventDraft>>, () => void] {
  const [workspace] = useState(() => supplied || resolveWorkspace()),
    [draft, setDraft] = useState(() => loadDraft(localStorage, workspace)),
    latest = useRef(draft),
    skip = useRef(false),
    cleared = useRef(false);
  latest.current = draft;

  useEffect(() => {
    if (skip.current) {
      skip.current = false;
      return;
    }
    cleared.current = false;
    const timer = window.setTimeout(() => saveSafely(latest.current, workspace), delay);
    return () => window.clearTimeout(timer);
  }, [draft, workspace]);

  useEffect(() => {
    const flush = () => saveSafely(latest.current, workspace);
    const visibility = () => {
      if (document.hidden) flush();
    };
    addEventListener("pagehide", flush);
    document.addEventListener("visibilitychange", visibility);
    return () => {
      removeEventListener("pagehide", flush);
      document.removeEventListener("visibilitychange", visibility);
      if (!cleared.current) flush();
    };
  }, [workspace]);

  const clear = () => {
    skip.current = true;
    cleared.current = true;
    localStorage.removeItem(draftKey(workspace));
    localStorage.removeItem(legacyDraft);
    setDraft({ ...initialDraft });
  };
  return [draft, setDraft, clear];
}

export function useOptions(supplied?: string): [Options, Dispatch<SetStateAction<string>>, Dispatch<SetStateAction<string>>] {
  const [workspace] = useState(() => supplied || resolveWorkspace()),
    [loaded] = useState(() => loadOptions(localStorage, workspace)),
    [mode, setMode] = useState(loaded.mode),
    [layout, setLayout] = useState(loaded.layout);
  useEffect(() => {
    const timer = window.setTimeout(() => saveOptions(localStorage, { mode, layout }, workspace), delay);
    return () => window.clearTimeout(timer);
  }, [layout, mode, workspace]);
  return [{ mode, layout }, setMode, setLayout];
}

function saveSafely(draft: EventDraft, workspace: string) {
  try {
    saveDraft(localStorage, draft, workspace);
  } catch {
    // Local storage can be unavailable in private browsing; the live draft still works
  }
}

function normalise(value: Record<string, unknown>): EventDraft {
  const draft = { ...initialDraft };
  for (const key of stringFields) if (typeof value[key] === "string") draft[key] = value[key] as never;
  for (const key of booleanFields) if (typeof value[key] === "boolean") draft[key] = value[key] as never;
  return draft;
}

function normaliseOptions(value: unknown): Options {
  const source = record(value) ? value : {};
  return {
    mode: ["both", "risk", "form"].includes(String(source.mode)) ? String(source.mode) : "both",
    layout: ["form", "pack", "both"].includes(String(source.layout)) ? String(source.layout) : "form",
  };
}

function isEnvelope(value: unknown, workspace: string): value is DraftEnvelope {
  return record(value)
    && value.version === 2
    && value.workspace === workspace
    && value.mini === "eventy"
    && value.form === "prepare"
    && value.schema === schema
    && typeof value.savedAt === "string"
    && record(value.values);
}

function isOptions(value: unknown, workspace: string): value is { values: Options } {
  return record(value)
    && value.version === 2
    && value.workspace === workspace
    && value.mini === "eventy"
    && value.form === "options"
    && value.schema === optionSchema
    && typeof value.savedAt === "string"
    && record(value.values);
}

function parse(value: string | null): unknown {
  if (!value || bytes(value) > limit) return null;
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function bytes(value: string) {
  return new TextEncoder().encode(value).byteLength;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
