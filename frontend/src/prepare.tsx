import { useRef, useState, type Dispatch, type FormEvent, type SetStateAction } from "react";
import { prepare, rooms } from "./api";
import type { Account, EventDraft, Prepared, Room } from "./model";

const steps = ["Event", "Venue", "Timing", "People", "Shape", "Prepare"];

export function Prepare({
  draft,
  setDraft,
  account,
}: {
  draft: EventDraft;
  setDraft: Dispatch<SetStateAction<EventDraft>>;
  account: Account | null;
}) {
  const [step, setStep] = useState(0),
    [mode, setMode] = useState("both"),
    [layout, setLayout] = useState("form"),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [result, setResult] = useState<Prepared | null>(null),
    [reviewed, setReviewed] = useState(false),
    form = useRef<HTMLFormElement>(null);

  const update = <K extends keyof EventDraft>(key: K, value: EventDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));

  const next = (event: FormEvent) => {
    event.preventDefault();
    if (!form.current?.reportValidity()) return;
    setStep((current) => Math.min(current + 1, steps.length - 1));
  };

  const make = async (submit = false) => {
    setBusy(true);
    setError("");
    try {
      const prepared = await prepare(draft, mode, layout, submit);
      setResult(prepared);
      setReviewed(false);
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="journey">
      <aside className="steps" aria-label="Event preparation steps">
        <p>Event route</p>
        <ol>
          {steps.map((label, index) => (
            <li key={label} data-done={index < step || undefined}>
              <button
                type="button"
                aria-current={index === step ? "step" : undefined}
                onClick={() => setStep(index)}
              >
                <span>{index + 1}</span>
                {label}
              </button>
            </li>
          ))}
        </ol>
      </aside>

      <div className="step-panel">
        {error && <Popup message={error} close={() => setError("")} />}
        <form ref={form} onSubmit={next}>
          {step === 0 && (
            <Step title="Name the gathering" note="Start with what people need to recognise">
              <Field label="Club or society">
                <input value={draft.club} onChange={(event) => update("club", event.target.value)} required />
              </Field>
              <div className="field-row">
                <Field label="Event name">
                  <input value={draft.title} onChange={(event) => update("title", event.target.value)} required />
                </Field>
                <Field label="Date">
                  <input type="date" value={draft.date} onChange={(event) => update("date", event.target.value)} required />
                </Field>
              </div>
              <Field label="What will happen?">
                <textarea
                  value={draft.summary}
                  onChange={(event) => update("summary", event.target.value)}
                  rows={5}
                  required
                />
              </Field>
            </Step>
          )}

          {step === 1 && <Venue draft={draft} update={update} />}

          {step === 2 && (
            <Step title="Set the tide" note="Include setup and pack-up so the room request is accurate">
              <TimeRow
                title="Setup"
                start={draft.setupStart}
                end={draft.setupEnd}
                setStart={(value) => update("setupStart", value)}
                setEnd={(value) => update("setupEnd", value)}
              />
              <TimeRow
                title="Event"
                start={draft.eventStart}
                end={draft.eventEnd}
                setStart={(value) => update("eventStart", value)}
                setEnd={(value) => update("eventEnd", value)}
              />
              <TimeRow
                title="Pack-up"
                start={draft.packStart}
                end={draft.packEnd}
                setStart={(value) => update("packStart", value)}
                setEnd={(value) => update("packEnd", value)}
              />
            </Step>
          )}

          {step === 3 && (
            <Step title="Name the coordinator" note="This is the person UQ can contact about the booking">
              <div className="field-row">
                <Field label="Coordinator name">
                  <input value={draft.coordinator} onChange={(event) => update("coordinator", event.target.value)} required />
                </Field>
                <Field label="Role">
                  <input value={draft.role} onChange={(event) => update("role", event.target.value)} required />
                </Field>
              </div>
              <div className="field-row">
                <Field label="Email">
                  <input type="email" value={draft.email} onChange={(event) => update("email", event.target.value)} required />
                </Field>
                <Field label="Phone">
                  <input type="tel" value={draft.phone} onChange={(event) => update("phone", event.target.value)} required />
                </Field>
              </div>
              <div className="field-row three">
                <Field label="Expected guests">
                  <input min="1" type="number" value={draft.attendance} onChange={(event) => update("attendance", event.target.value)} required />
                </Field>
                <Field label="Under 18">
                  <input min="0" type="number" value={draft.under18} onChange={(event) => update("under18", event.target.value)} required />
                </Field>
                <Field label="Outside UQ">
                  <input min="0" type="number" value={draft.nonUq} onChange={(event) => update("nonUq", event.target.value)} required />
                </Field>
              </div>
            </Step>
          )}

          {step === 4 && (
            <Step title="Describe the event shape" note="These choices build the first risk assessment for review">
              <div className="choice-grid">
                {(
                  [
                    ["indoor", "Indoors"],
                    ["seated", "Mainly seated"],
                    ["electrical", "Electrical equipment"],
                    ["vehicles", "Vehicles"],
                    ["structures", "Temporary structures"],
                    ["cash", "Cash handling"],
                    ["alcohol", "Alcohol"],
                    ["competition", "Physical competition"],
                    ["offSite", "Activity outside the room"],
                  ] as [keyof EventDraft, string][]
                ).map(([key, label]) => (
                  <label className="choice" key={key}>
                    <input
                      type="checkbox"
                      checked={Boolean(draft[key])}
                      onChange={(event) => update(key, event.target.checked as never)}
                    />
                    <span>{label}</span>
                  </label>
                ))}
              </div>
              <Field label="Food and drink">
                <input value={draft.food} onChange={(event) => update("food", event.target.value)} placeholder="Leave blank if none" />
              </Field>
              <div className="field-row">
                <Field label="UQ affiliated">
                  <select value={draft.affiliated} onChange={(event) => update("affiliated", event.target.value)}>
                    <option>Yes</option>
                    <option>No</option>
                  </select>
                </Field>
                <Field label="Other dates that work">
                  <input value={draft.flexibility} onChange={(event) => update("flexibility", event.target.value)} />
                </Field>
              </div>
            </Step>
          )}

          {step === 5 && (
            <Step title="Prepare the event" note="Files stay editable, and booking submission remains a separate decision">
              <div className="output-choices">
                <fieldset>
                  <legend>Make</legend>
                  {[
                    ["both", "Booking preview + risk assessment"],
                    ["risk", "Risk assessment only"],
                    ["form", "Booking preview only"],
                  ].map(([value, label]) => (
                    <label key={value}>
                      <input type="radio" name="mode" value={value} checked={mode === value} onChange={() => setMode(value)} />
                      {label}
                    </label>
                  ))}
                </fieldset>
                {mode !== "form" && (
                  <fieldset>
                    <legend>Risk document</legend>
                    {[
                      ["form", "Editable UQ form"],
                      ["pack", "Event pack"],
                      ["both", "Both"],
                    ].map(([value, label]) => (
                      <label key={value}>
                        <input type="radio" name="layout" value={value} checked={layout === value} onChange={() => setLayout(value)} />
                        {label}
                      </label>
                    ))}
                  </fieldset>
                )}
              </div>
              <button className="primary prepare-button" type="button" disabled={busy} onClick={() => void make()}>
                {busy ? "Preparing…" : result ? "Prepare fresh files" : "Prepare files"}
              </button>
              {result && (
                <div className="landing" aria-live="polite">
                  <div className="landing-head">
                    <span aria-hidden="true">✓</span>
                    <div>
                      <h3>{result.title} is ready</h3>
                      <p>Open the editable files and check the booking preview</p>
                    </div>
                  </div>
                  <div className="downloads">
                    {result.downloads.map((file) => (
                      <a key={file.url} href={new URL(file.url, document.baseURI).toString()} download>
                        <FileIcon kind={file.kind} />
                        <span>{file.name}</span>
                      </a>
                    ))}
                  </div>
                  {result.preview != null && (
                    <details>
                      <summary>Review booking answers</summary>
                      <pre>{JSON.stringify(result.preview, null, 2)}</pre>
                    </details>
                  )}
                  {mode !== "risk" && (
                    <div className="submit-booking">
                      {account?.forms_ready ? (
                        <>
                          <label>
                            <input type="checkbox" checked={reviewed} onChange={(event) => setReviewed(event.target.checked)} />
                            I checked this booking and want to send it now
                          </label>
                          <button type="button" disabled={!reviewed || busy} onClick={() => void make(true)}>
                            {busy ? "Sending…" : "Send booking"}
                          </button>
                        </>
                      ) : (
                        <p>Booking preview is available. Submission will appear when the private Forms connection is configured</p>
                      )}
                    </div>
                  )}
                </div>
              )}
            </Step>
          )}

          <div className="step-actions">
            {step > 0 && (
              <button className="quiet" type="button" onClick={() => setStep((current) => current - 1)}>
                Back
              </button>
            )}
            {step < steps.length - 1 && <button type="submit">Continue</button>}
          </div>
        </form>
      </div>
    </div>
  );
}

function Venue({
  draft,
  update,
}: {
  draft: EventDraft;
  update: <K extends keyof EventDraft>(key: K, value: EventDraft[K]) => void;
}) {
  const [query, setQuery] = useState(""),
    [matches, setMatches] = useState<Room[]>([]),
    [searching, setSearching] = useState(false),
    [error, setError] = useState("");

  const search = async () => {
    setSearching(true);
    setError("");
    try {
      setMatches(await rooms(query, Number(draft.attendance) || 0));
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setSearching(false);
    }
  };

  const select = (room: Room) => {
    update("campus", room.campus);
    update("building", room.building);
    update("room", room.room);
    update("roomSeats", String(room.seats));
  };

  return (
    <Step title="Find a landing place" note="Search by campus, building, room type or capacity">
      <div className="room-search">
        <input
          aria-label="Room search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Try Steele, collaborative, or 50 people"
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void search();
            }
          }}
        />
        <button type="button" onClick={() => void search()} disabled={searching}>
          {searching ? "Searching…" : "Find rooms"}
        </button>
      </div>
      {error && <p className="inline-error" role="alert">{error}</p>}
      {matches.length > 0 && (
        <ul className="room-results" aria-label="Matching rooms">
          {matches.map((room) => (
            <li key={`${room.campus}-${room.room}`}>
              <button type="button" onClick={() => select(room)}>
                <span>
                  <strong>{room.room}</strong>
                  {room.building}
                </span>
                <span>{room.seats} seats · {room.kind}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="selected-room" data-selected={Boolean(draft.room) || undefined}>
        <span aria-hidden="true">⌖</span>
        <div>
          <strong>{draft.room || "No room selected"}</strong>
          <p>{draft.room ? `${draft.building}, ${draft.campus} · ${draft.roomSeats} seats` : "Search above or enter the venue details"}</p>
        </div>
      </div>
      <div className="field-row three">
        <Field label="Campus">
          <input value={draft.campus} onChange={(event) => update("campus", event.target.value)} required />
        </Field>
        <Field label="Building">
          <input value={draft.building} onChange={(event) => update("building", event.target.value)} required />
        </Field>
        <Field label="Room">
          <input value={draft.room} onChange={(event) => update("room", event.target.value)} required />
        </Field>
      </div>
    </Step>
  );
}

function Step({ title, note, children }: { title: string; note: string; children: React.ReactNode }) {
  return (
    <fieldset className="step-fields">
      <legend>{title}</legend>
      <p className="step-note">{note}</p>
      {children}
    </fieldset>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

function TimeRow({
  title,
  start,
  end,
  setStart,
  setEnd,
}: {
  title: string;
  start: string;
  end: string;
  setStart: (value: string) => void;
  setEnd: (value: string) => void;
}) {
  return (
    <fieldset className="time-row">
      <legend>{title}</legend>
      <label>
        <span>Starts</span>
        <input type="time" value={start} onChange={(event) => setStart(event.target.value)} required />
      </label>
      <span aria-hidden="true">to</span>
      <label>
        <span>Ends</span>
        <input type="time" value={end} onChange={(event) => setEnd(event.target.value)} required />
      </label>
    </fieldset>
  );
}

function Popup({ message, close }: { message: string; close: () => void }) {
  return (
    <div className="popup-shade">
      <div className="popup" role="alertdialog" aria-modal="true" aria-labelledby="problem-title">
        <span className="popup-bird" aria-hidden="true">⌁</span>
        <h2 id="problem-title">That part needs attention</h2>
        <p>{message}</p>
        <button type="button" autoFocus onClick={close}>Back to the event</button>
      </div>
    </div>
  );
}

function FileIcon({ kind }: { kind: string }) {
  return <span className="file-icon" aria-hidden="true">{kind === "source" ? "{}" : kind === "form" ? "↗" : "W"}</span>;
}
