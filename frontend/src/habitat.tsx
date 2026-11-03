export function DockMark({ kind }: { kind: "eventy" | "monkey" | "meddy" }) {
  if (kind === "eventy") return <Mark />;
  if (kind === "monkey") {
    return (
      <svg className="mini-mark" viewBox="0 0 64 64" aria-hidden="true">
        <path d="M16 7h25l8 8v42H16Z M41 7v10h8 M23 27h19 M23 36h13" />
        <path d="m34 48 4 4 9-11" />
      </svg>
    );
  }
  return (
    <svg className="mini-mark" viewBox="0 0 64 64" aria-hidden="true">
      <path d="M10 22h15l18-11v42L25 42H10Z M10 22v20 M25 22v20" />
      <path d="M49 22c4 6 4 14 0 20 M55 16c8 10 8 22 0 32" />
    </svg>
  );
}

export function Mark() {
  return (
    <svg className="mini-mark" viewBox="0 0 64 64" aria-hidden="true">
      <path d="M7 27c9-8 18-9 28-2 7 5 14 5 22 0-4 11-13 17-25 17S12 37 7 27Z" />
      <path d="M21 25C18 15 13 10 7 7c11 0 20 4 27 13-3 4-8 6-13 5Z" />
      <path d="M9 51c7-4 14-4 21 0 7 4 14 4 21 0" />
    </svg>
  );
}
