import { useEffect, useId, useRef } from "react";

export function ErrorDialog({
  message,
  close,
  retry,
  closeLabel = "Back to the event",
}: {
  message: string;
  close: () => void;
  retry?: () => void;
  closeLabel?: string;
}) {
  const title = useId(),
    description = useId(),
    button = useRef<HTMLButtonElement>(null),
    closeRef = useRef(close);
  closeRef.current = close;

  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    button.current?.focus();
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeRef.current();
    };
    addEventListener("keydown", escape);
    return () => {
      removeEventListener("keydown", escape);
      previous?.focus();
    };
  }, []);

  return (
    <div className="popup-shade">
      <div className="popup" role="alertdialog" aria-modal="true" aria-labelledby={title} aria-describedby={description}>
        <span className="popup-bird" aria-hidden="true">⌁</span>
        <h2 id={title}>That part needs attention</h2>
        <p id={description}>{message}</p>
        <div className="popup-actions">
          {retry && <button type="button" className="quiet" onClick={retry}>Try again</button>}
          <button ref={button} type="button" onClick={close}>{closeLabel}</button>
        </div>
      </div>
    </div>
  );
}
