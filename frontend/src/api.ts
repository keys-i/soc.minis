import type { Account, Booking, EventDraft, Prepared, Room } from "./model";
import { eventToml } from "./model";

const endpoint = (path: string) => `${import.meta.env.BASE_URL}api/${path}`;

async function request(path: string, init?: RequestInit): Promise<unknown> {
  const response = await fetch(endpoint(path), {
    ...init,
    headers: { "Content-Type": "application/json", ...init?.headers },
  });
  let value: unknown;
  try {
    value = await response.json();
  } catch {
    throw new Error("SOC Minis returned an unreadable response. Try again shortly");
  }
  if (!response.ok) {
    const message = record(value) && typeof value.error === "string" ? value.error : "That didn’t work";
    throw new Error(message);
  }
  return value;
}

export async function status(): Promise<Account> {
  const value = await request("status");
  if (
    !record(value) ||
    !(value.microsoft_mailbox === null || typeof value.microsoft_mailbox === "string") ||
    typeof value.microsoft_ready !== "boolean" ||
    typeof value.forms_ready !== "boolean"
  )
    throw malformed();
  return value as Account;
}

export async function rooms(query: string, minimum: number): Promise<Room[]> {
  const value = await request(
    `rooms?q=${encodeURIComponent(query)}&minimum=${encodeURIComponent(minimum)}`,
  );
  if (!Array.isArray(value) || !value.every(room)) throw malformed();
  return value;
}

export async function prepare(
  draft: EventDraft,
  mode: string,
  layout: string,
  submit = false,
): Promise<Prepared> {
  const value = await request("event", {
    method: "POST",
    body: JSON.stringify({
      source: eventToml(draft, mode, layout),
      mode,
      layout,
      submit,
      authorised: submit,
    }),
  });
  if (
    !record(value) ||
    typeof value.title !== "string" ||
    typeof value.submitted !== "boolean" ||
    !Array.isArray(value.downloads)
  )
    throw malformed();
  return value as Prepared;
}

export async function startMicrosoft(mailbox: string) {
  const value = await request("microsoft/start", {
    method: "POST",
    body: JSON.stringify({ mailbox }),
  });
  if (
    !record(value) ||
    typeof value.id !== "string" ||
    typeof value.user_code !== "string" ||
    typeof value.verification_uri !== "string" ||
    typeof value.interval_seconds !== "number"
  )
    throw malformed();
  return value as {
    id: string;
    user_code: string;
    verification_uri: string;
    interval_seconds: number;
    message: string;
  };
}

export async function pollMicrosoft(id: string) {
  const value = await request(`microsoft/poll/${encodeURIComponent(id)}`, {
    method: "POST",
    body: "{}",
  });
  if (!record(value) || typeof value.status !== "string") throw malformed();
  return value as { status: "pending"; retry_after: number } | { status: "connected"; mailbox: string };
}

export async function bookings(): Promise<Booking[]> {
  const value = await request("agent/status");
  if (!Array.isArray(value) || !value.every(booking)) throw malformed();
  return value;
}

export async function watch(draft: EventDraft, eventId: string): Promise<Booking> {
  const value = await request("agent/watch", {
    method: "POST",
    body: JSON.stringify({ source: eventToml(draft), event_id: eventId }),
  });
  if (!booking(value)) throw malformed();
  return value;
}

export async function act(action: "stop" | "retry", eventId: string): Promise<Booking[]> {
  const value = await request(`agent/${action}`, {
    method: "POST",
    body: JSON.stringify({ event_id: eventId, confirm_not_sent: action === "retry" }),
  });
  if (!Array.isArray(value) || !value.every(booking)) throw malformed();
  return value;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function room(value: unknown): value is Room {
  return (
    record(value) &&
    typeof value.campus === "string" &&
    typeof value.building === "string" &&
    typeof value.room === "string" &&
    typeof value.kind === "string" &&
    typeof value.seats === "number" &&
    typeof value.tour === "string" &&
    typeof value.photo === "string"
  );
}

function booking(value: unknown): value is Booking {
  return (
    record(value) &&
    ["event_id", "title", "state", "outcome", "details", "link", "error", "updated_at"].every(
      (key) => typeof value[key] === "string",
    )
  );
}

function malformed() {
  return new Error("SOC Minis returned unexpected data. Try again shortly");
}
