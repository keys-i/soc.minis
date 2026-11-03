import { DockMark } from "./habitat";
import { minis } from "./minis";
import { miniHref } from "./route";
import { World } from "./world";

export function Home() {
  return (
    <main id="main" className="home-main" tabIndex={-1}>
      <section className="society-station" aria-labelledby="home-title">
        <World />
        <div className="station-copy">
          <h1 id="home-title">
            Keep the good chaos.
            <br />
            Sort the admin.
          </h1>
          <p>Choose a dock. Each mini opens where its work happens.</p>
        </div>

        <nav className="mini-landmarks" aria-label="SOC Minis">
          {minis.map((mini) => (
            <a
              className={`mini-landmark landmark-${mini.id}`}
              href={miniHref(mini.id)}
              key={mini.id}
            >
              <span className="landmark-mark">
                <DockMark kind={mini.id} />
              </span>
              <span>
                <small>{mini.status}</small>
                <strong>{mini.title}</strong>
                <span>{mini.summary}</span>
              </span>
            </a>
          ))}
        </nav>
      </section>
    </main>
  );
}
