import { useEffect, useState, type FormEvent } from "react";
import { act, bookings, pollMicrosoft, startMicrosoft, watch } from "./api";
import type { Account, Booking, EventDraft } from "./model";

export function Follow({
  draft,
  account,
  refreshAccount,
  prepare,
}: {
  draft: EventDraft;
  account: Account | null;
  refreshAccount: () => Promise<void>;
  prepare: () => void;
}) {
  const [rows, setRows] = useState<Booking[]>([]),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(true),
    [mailbox, setMailbox] = useState(""),
    [flow, setFlow] = useState<Awaited<ReturnType<typeof startMicrosoft>> | null>(null),
    [eventId, setEventId] = useState("");

  const refresh = async () => {
    try {
      setRows(await bookings());
      setError("");
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 15_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!flow) return;
    let active = true;
    const check = async () => {
      try {
        const result = await pollMicrosoft(flow.id);
        if (!active) return;
        if (result.status === "connected") {
          setFlow(null);
          await refreshAccount();
          return;
        }
        window.setTimeout(check, Math.max(1, result.retry_after) * 1000);
      } catch (failure) {
        if (active) setError((failure as Error).message);
      }
    };
    const timer = window.setTimeout(check, Math.max(1, flow.interval_seconds) * 1000);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [flow]);

  const connect = async (event: FormEvent) => {
    event.preventDefault();
    setError("");
    try {
      setFlow(await startMicrosoft(mailbox));
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  const follow = async (event: FormEvent) => {
    event.preventDefault();
    setError("");
    try {
      const row = await watch(draft, eventId);
      setRows((current) => [row, ...current.filter((item) => item.event_id !== row.event_id)]);
      setEventId("");
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  const action = async (kind: "stop" | "retry", id: string) => {
    setError("");
    try {
      setRows(await act(kind, id));
    } catch (failure) {
      setError((failure as Error).message);
    }
  };

  return (
    <div className="follow-grid">
      <section className="mail-connection" aria-labelledby="mail-heading">
        <div className="section-title">
          <span className="section-icon" aria-hidden="true">✉</span>
          <div>
            <h2 id="mail-heading">Booking mailbox</h2>
            <p>Replies are checked automatically</p>
          </div>
        </div>
        {error && <div className="inline-error" role="alert">{error}</div>}
        {account?.microsoft_mailbox ? (
          <div className="connected-account">
            <span aria-hidden="true">✓</span>
            <div>
              <strong>{account.microsoft_mailbox}</strong>
              <p>Connected to Microsoft 365</p>
            </div>
          </div>
        ) : flow ? (
          <div className="device-flow" aria-live="polite">
            <p>Open Microsoft, then enter this code</p>
            <strong>{flow.user_code}</strong>
            <a href={flow.verification_uri} target="_blank" rel="noreferrer">Open Microsoft sign-in</a>
            <small>Waiting here for Microsoft to finish…</small>
          </div>
        ) : (
          <form className="connect-form" onSubmit={connect}>
            <label>
              <span>Society mailbox</span>
              <input type="email" value={mailbox} onChange={(event) => setMailbox(event.target.value)} placeholder="events@example.org" required />
            </label>
            <button type="submit" disabled={!account?.microsoft_ready}>Connect Microsoft</button>
            {account && !account.microsoft_ready && <p>The Microsoft app registration needs configuring on the server</p>}
          </form>
        )}
      </section>

      <section className="watch-setup" aria-labelledby="watch-heading">
        <div className="section-title">
          <span className="section-icon fish-icon" aria-hidden="true">◖</span>
          <div>
            <h2 id="watch-heading">Follow a booking</h2>
            <p>The risk return is sent only for the event you add</p>
          </div>
        </div>
        {draft.title ? (
          <form onSubmit={follow}>
            <div className="event-chip">
              <strong>{draft.title}</strong>
              <span>{draft.date || "Date not set"} · {draft.room || "Room not set"}</span>
            </div>
            <label>
              <span>UQ event reference</span>
              <input value={eventId} onChange={(event) => setEventId(event.target.value)} placeholder="Reference ID" pattern="[A-Za-z0-9_-]{1,80}" required />
            </label>
            <button type="submit" disabled={!account?.microsoft_mailbox}>Follow this booking</button>
          </form>
        ) : (
          <div className="empty-small">
            <p>Prepare the event details before following its booking</p>
            <button type="button" onClick={prepare}>Prepare event</button>
          </div>
        )}
      </section>

      <section className="booking-list" aria-labelledby="bookings-heading">
        <div className="list-head">
          <div>
            <h2 id="bookings-heading">Bookings in flight</h2>
            <p aria-live="polite">{loading ? "Checking…" : `${rows.length} ${rows.length === 1 ? "booking" : "bookings"}`}</p>
          </div>
          <span className="live-dot">Updating automatically</span>
        </div>
        {!loading && rows.length === 0 ? (
          <div className="ocean-empty">
            <span aria-hidden="true">≈ &gt;°)</span>
            <p>No booking replies are being followed yet</p>
          </div>
        ) : (
          <ul>
            {rows.map((row) => (
              <li key={row.event_id}>
                <span className="state-mark" data-state={row.state} aria-hidden="true" />
                <div className="booking-main">
                  <strong>{row.title}</strong>
                  <span>{row.event_id}</span>
                  <p>{row.error || row.details || stateLabel(row)}</p>
                  {row.link && <a href={row.link} target="_blank" rel="noreferrer">Open Outlook message</a>}
                </div>
                <div className="booking-side">
                  <span>{stateLabel(row)}</span>
                  <time dateTime={row.updated_at}>{date(row.updated_at)}</time>
                  {!(["complete", "stopped"] as string[]).includes(row.state) && (
                    <button className="quiet" type="button" onClick={() => void action("stop", row.event_id)}>Stop</button>
                  )}
                  {row.state === "sending" && row.error.includes("uncertain") && (
                    <button type="button" onClick={() => void action("retry", row.event_id)}>Checked Sent Items · retry</button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

function stateLabel(row: Booking) {
  if (row.outcome) return row.outcome.replaceAll("_", " ");
  return (
    {
      waiting_request: "Waiting for UQ request",
      draft: "Preparing risk return",
      sending: "Sending risk return",
      sent: "Waiting for booking update",
      complete: "Complete",
      stopped: "Stopped",
    }[row.state] || row.state.replaceAll("_", " ")
  );
}

function date(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? "" : new Intl.DateTimeFormat("en-AU", { dateStyle: "medium", timeStyle: "short" }).format(date);
}
