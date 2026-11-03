import { homeHref } from "./route";
import { World } from "./world";

export function Lost() {
  return (
    <main id="main" className="lost-page" tabIndex={-1}>
      <section className="lost-island" aria-labelledby="lost-title">
        <World compact />
        <div className="lost-copy">
          <span aria-hidden="true">404</span>
          <h1 id="lost-title">That island isn't on the chart</h1>
          <p>The mini may have moved, or the address may be mistyped.</p>
          <a href={homeHref}>Return to SOC Minis</a>
        </div>
      </section>
    </main>
  );
}
