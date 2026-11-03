import { DockMark } from "./habitat";
import { miniById } from "./minis";
import { World } from "./world";

export function Preview({ mini }: { mini: "monkey" | "meddy" }) {
  const preview = miniById[mini];
  return (
    <main id="main" className={`mini-page preview-page preview-${mini}`} tabIndex={-1}>
      <section className={`mini-mast mini-mast-${mini}`} aria-labelledby={`${mini}-title`}>
        <World compact scene={mini} />
        <div className="mast-mark">
          <DockMark kind={mini} />
        </div>
        <div className="mast-copy">
          <span>{preview.status}</span>
          <h1 id={`${mini}-title`}>{preview.title}</h1>
          <p>{preview.intro}.</p>
        </div>
      </section>

      <section className="preview-flow" aria-labelledby={`${mini}-flow-title`}>
        <div className="preview-intro">
          <h2 id={`${mini}-flow-title`}>{preview.flowTitle}</h2>
          <p>{preview.scope}</p>
          <strong>{preview.note}</strong>
        </div>
        <ol>
          {preview.steps.map((step) => (
            <li key={step}>{step}</li>
          ))}
        </ol>
      </section>
    </main>
  );
}
