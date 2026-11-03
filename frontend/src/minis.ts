export type Mini = "eventy" | "monkey" | "meddy";

type MiniBase = {
  id: Mini;
  title: string;
  status: string;
  summary: string;
  intro: string;
};

type ReadyMini = MiniBase & { available: true };

type PreviewMini = MiniBase & {
  available: false;
  flowTitle: string;
  note: string;
  scope: string;
  steps: readonly string[];
};

export type MiniInfo = ReadyMini | PreviewMini;

export const minis = [
  {
    id: "eventy",
    title: "Eventy",
    status: "Ready now",
    summary: "Prepare the event and follow the booking",
    intro: "Prepare the event files, then keep the booking follow-up moving",
    available: true,
  },
  {
    id: "monkey",
    title: "Monkey",
    status: "Coming soon",
    summary: "Receipts, reimbursements and one traceable ledger",
    intro: "One accountable path from receipt to reconciled society ledger",
    available: false,
    flowTitle: "A clean paper trail",
    steps: ["Receipt arrives", "Executive checks it", "Reimbursement gets a number", "Ledger reconciles"],
    scope: "AFR and union exports, budgets, ticket modelling, bank matching and unusual-transaction review",
    note: "No bank or messaging account is connected yet",
  },
  {
    id: "meddy",
    title: "Meddy",
    status: "Coming soon",
    summary: "Plan one post for every society channel",
    intro: "Write once, then make the post fit each society channel",
    available: false,
    flowTitle: "One post, properly adapted",
    steps: ["Add the caption and media", "Review each channel", "Approve together", "Publish with a history"],
    scope: "Instagram, LinkedIn and Discord with media, approvals and channel previews",
    note: "No social account is connected yet",
  },
] as const satisfies readonly MiniInfo[];

export const miniById = Object.fromEntries(minis.map((mini) => [mini.id, mini])) as {
  [K in Mini]: Extract<(typeof minis)[number], { id: K }>;
};

export function isMini(value: string): value is Mini {
  return value === "eventy" || value === "monkey" || value === "meddy";
}
