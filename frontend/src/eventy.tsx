import { useEffect, useState } from "react";
import { status } from "./api";
import { ErrorDialog } from "./dialog";
import { useDraft } from "./draft";
import { Follow } from "./follow";
import { DockMark } from "./habitat";
import { miniById } from "./minis";
import type { Account } from "./model";
import { Prepare } from "./prepare";
import { World } from "./world";

export function Eventy() {
  const [account, setAccount] = useState<Account | null>(null),
    [accountError, setAccountError] = useState(""),
    [loading, setLoading] = useState(true);

  const refreshAccount = async () => {
    setLoading(true);
    try {
      setAccount(await status());
      setAccountError("");
    } catch (error) {
      setAccount(null);
      setAccountError((error as Error).message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refreshAccount();
  }, []);

  return (
    <main id="main" className="mini-page eventy-page" tabIndex={-1}>
      <section className="mini-mast mini-mast-eventy" aria-labelledby="eventy-title">
        <World compact />
        <div className="mast-mark">
          <DockMark kind="eventy" />
        </div>
        <div className="mast-copy">
          <span>{miniById.eventy.status}</span>
          <h1 id="eventy-title">{miniById.eventy.title}</h1>
          <p>{miniById.eventy.intro}.</p>
        </div>
      </section>

      {account ? (
        <Workspace account={account} refreshAccount={refreshAccount} />
      ) : (
        <section className="eventy-workspace eventy-gate" aria-label="Opening Eventy workspace">
          <div className="workspace-loading" role="status">
            <span aria-hidden="true" />
            <p>{loading ? "Opening your private workspace…" : "Your workspace is still closed"}</p>
            {!loading && !accountError && <button type="button" onClick={() => void refreshAccount()}>Try again</button>}
          </div>
          {accountError && <ErrorDialog message={accountError} close={() => setAccountError("")} closeLabel="Close" retry={() => void refreshAccount()} />}
        </section>
      )}
    </main>
  );
}

function Workspace({ account, refreshAccount }: { account: Account; refreshAccount: () => Promise<void> }) {
  const workspace = `hosted:${account.workspace}`,
    [draft, setDraft, clearDraft] = useDraft(workspace),
    [screen, setScreen] = useState<"prepare" | "follow">(initialScreen),
    [confirmClear, setConfirmClear] = useState(false),
    [revision, setRevision] = useState(0);

  const show = (next: "prepare" | "follow") => {
    setScreen(next);
    const url = new URL(window.location.href);
    if (next === "follow") url.searchParams.set("view", "follow");
    else url.searchParams.delete("view");
    history.replaceState(null, "", url);
  };

  const clear = () => {
    clearDraft();
    show("prepare");
    setRevision((current) => current + 1);
    setConfirmClear(false);
  };

  return (
    <section
      className="eventy-workspace"
      aria-label={screen === "prepare" ? "Prepare event" : "Booking follow-up"}
    >
      <div className="workbench-head">
        <div>
          <span>Eventy</span>
          <h2>{screen === "prepare" ? "Plan an event" : "Follow a booking"}</h2>
        </div>
        <div className="workbench-controls">
          <nav className="eventy-tabs" aria-label="Eventy workspace">
            <button type="button" aria-pressed={screen === "prepare"} onClick={() => show("prepare")}>Plan</button>
            <button type="button" aria-pressed={screen === "follow"} onClick={() => show("follow")}>Follow</button>
          </nav>
          {confirmClear ? (
            <div className="clear-confirm" role="group" aria-label="Clear saved Eventy draft">
              <span>Start over?</span>
              <button type="button" className="quiet" onClick={() => setConfirmClear(false)}>Keep it</button>
              <button type="button" onClick={clear}>Clear</button>
            </div>
          ) : (
            <button type="button" className="clear-draft quiet" onClick={() => setConfirmClear(true)}>Clear draft</button>
          )}
        </div>
      </div>
      {screen === "prepare" ? (
        <Prepare key={revision} draft={draft} setDraft={setDraft} account={account} />
      ) : (
        <Follow
          draft={draft}
          account={account}
          refreshAccount={refreshAccount}
          prepare={() => show("prepare")}
        />
      )}
    </section>
  );
}

function initialScreen(): "prepare" | "follow" {
  return new URLSearchParams(window.location.search).get("view") === "follow"
    ? "follow"
    : "prepare";
}
