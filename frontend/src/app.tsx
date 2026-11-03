import { useEffect, useState } from "react";
import { status } from "./api";
import { Follow } from "./follow";
import { Habitat, Mark } from "./habitat";
import { initialDraft, type Account, type EventDraft } from "./model";
import { Prepare } from "./prepare";

const storage = "soc-minis-event-v1";

export function App() {
  const [screen, setScreen] = useState<"prepare" | "follow">("prepare"),
    [draft, setDraft] = useState<EventDraft>(loadDraft),
    [account, setAccount] = useState<Account | null>(null),
    [accountError, setAccountError] = useState("");

  useEffect(() => {
    localStorage.setItem(storage, JSON.stringify(draft));
  }, [draft]);

  const refreshAccount = async () => {
    try {
      setAccount(await status());
      setAccountError("");
    } catch (error) {
      setAccountError((error as Error).message);
    }
  };

  useEffect(() => {
    void refreshAccount();
  }, []);

  return (
    <div className="site-shell">
      <header className="site-head">
        <a className="brand" href={import.meta.env.BASE_URL} aria-label="SOC Minis home">
          <span className="brand-mark">
            <Mark />
          </span>
          <span>SOC Minis</span>
        </a>
        <nav aria-label="SOC Minis tools">
          <button
            type="button"
            aria-current={screen === "prepare" ? "page" : undefined}
            onClick={() => setScreen("prepare")}
          >
            Prepare event
          </button>
          <button
            type="button"
            aria-current={screen === "follow" ? "page" : undefined}
            onClick={() => setScreen("follow")}
          >
            Booking follow-up
          </button>
        </nav>
      </header>

      <main>
        <section className="hero" aria-labelledby="hero-title">
          <Habitat />
          <div className="hero-copy">
            <h1 id="hero-title">From first flight to safe return</h1>
            <p>Prepare the event, then let booking follow-up run quietly in the background</p>
          </div>
        </section>

        <section className="workbench" aria-label={screen === "prepare" ? "Prepare event" : "Booking follow-up"}>
          {accountError && (
            <div className="notice error" role="alert">
              <strong>Connection needs attention</strong>
              <span>{accountError}</span>
              <button type="button" onClick={() => void refreshAccount()}>
                Try again
              </button>
            </div>
          )}
          {screen === "prepare" ? (
            <Prepare draft={draft} setDraft={setDraft} account={account} />
          ) : (
            <Follow
              draft={draft}
              account={account}
              refreshAccount={refreshAccount}
              prepare={() => setScreen("prepare")}
            />
          )}
        </section>
      </main>
      <footer>SOC Minis keeps event files and mailbox authority in your private workspace</footer>
    </div>
  );
}

function loadDraft(): EventDraft {
  try {
    const saved = JSON.parse(localStorage.getItem(storage) || "null");
    if (saved && typeof saved === "object") return { ...initialDraft, ...saved };
  } catch {
    localStorage.removeItem(storage);
  }
  return initialDraft;
}
