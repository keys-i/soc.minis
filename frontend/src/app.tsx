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
      <a className="skip-link" href="#eventy-workspace">Skip to Eventy</a>
      <header className="site-head">
        <a className="brand" href={import.meta.env.BASE_URL} aria-label="SOC Minis home">
          <span className="brand-mark">
            <Mark />
          </span>
          <span>SOC Minis</span>
        </a>
        <span className="head-note">Made for society teams</span>
      </header>

      <main>
        <section className="hero" aria-labelledby="hero-title">
          <Habitat />
          <div className="hero-layout">
            <div className="hero-copy">
              <p className="eyebrow">Club operations, brought ashore</p>
              <h1 id="hero-title">Run the club. Keep the chaos out.</h1>
              <p>Eventy is ready today. Monkey and Meddy are being shaped around the work that still lives in receipts, spreadsheets and too many logins.</p>
            </div>

            <section className="product-shelf" aria-label="SOC Minis products">
              <article className="product eventy-product">
                <div className="product-head">
                  <span className="product-icon eventy-icon" aria-hidden="true"><Mark /></span>
                  <span className="product-state available">Available now</span>
                </div>
                <h2>Eventy</h2>
                <p className="product-pitch">Events, without the admin fog</p>
                <p>Plan the gathering, prepare editable risk files and quietly follow the booking</p>
                <nav className="product-actions" aria-label="Eventy views">
                  <a
                    href="#eventy-workspace"
                    aria-current={screen === "prepare" ? "page" : undefined}
                    onClick={() => setScreen("prepare")}
                  >
                    Plan an event
                  </a>
                  <a
                    href="#eventy-workspace"
                    aria-current={screen === "follow" ? "page" : undefined}
                    onClick={() => setScreen("follow")}
                  >
                    Follow a booking
                  </a>
                </nav>
              </article>

              <article className="product preview-product monkey-product" aria-disabled="true">
                <div className="product-head">
                  <span className="product-icon" aria-hidden="true">▦</span>
                  <span className="product-state">Coming soon</span>
                </div>
                <h2>Monkey</h2>
                <p className="product-pitch">Money you can trace</p>
                <p>Receipts in, executive approval clear and every reimbursement numbered</p>
                <ul>
                  <li>Receipt intake from WhatsApp, Instagram and Messenger, with executive verification</li>
                  <li>Reimbursements, numbered references, AFR and union exports, budgets and ticket modelling</li>
                  <li>Read-only bank matching, spending insights, unusual-transaction review and tamper-evident role history</li>
                </ul>
              </article>

              <article className="product preview-product meddy-product" aria-disabled="true">
                <div className="product-head">
                  <span className="product-icon" aria-hidden="true">↗</span>
                  <span className="product-state">Coming soon</span>
                </div>
                <h2>Meddy</h2>
                <p className="product-pitch">One post, properly adapted</p>
                <p>Compose captions and media once, adapt each destination, then publish to Instagram, LinkedIn and Discord</p>
                <p className="product-detail">Team approvals · channel previews · publish history</p>
              </article>
            </section>
          </div>
        </section>

        <p className="preview-note">Monkey and Meddy are product previews. No bank, messaging or social account is connected yet.</p>

        <section
          className="workbench"
          id="eventy-workspace"
          tabIndex={-1}
          aria-label={screen === "prepare" ? "Prepare event" : "Booking follow-up"}
        >
          <div className="workbench-head">
            <span>Eventy</span>
            <strong>{screen === "prepare" ? "Plan an event" : "Follow a booking"}</strong>
          </div>
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
      <footer>Event files and mailbox authority stay in your private workspace</footer>
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
