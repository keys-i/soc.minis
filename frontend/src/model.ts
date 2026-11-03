export type EventDraft = {
  club: string;
  title: string;
  date: string;
  summary: string;
  attendance: string;
  campus: string;
  building: string;
  room: string;
  roomSeats: string;
  coordinator: string;
  role: string;
  email: string;
  phone: string;
  setupStart: string;
  setupEnd: string;
  eventStart: string;
  eventEnd: string;
  packStart: string;
  packEnd: string;
  food: string;
  affiliated: string;
  flexibility: string;
  under18: string;
  nonUq: string;
  indoor: boolean;
  seated: boolean;
  vehicles: boolean;
  structures: boolean;
  electrical: boolean;
  cash: boolean;
  alcohol: boolean;
  competition: boolean;
  offSite: boolean;
};

export type Room = {
  campus: string;
  building: string;
  room: string;
  kind: string;
  seats: number;
  tour: string;
  photo: string;
};

export type FileLink = { kind: string; name: string; url: string };

export type Prepared = {
  title: string;
  submitted: boolean;
  preview: unknown;
  downloads: FileLink[];
};

export type Account = {
  microsoft_mailbox: string | null;
  microsoft_ready: boolean;
  forms_ready: boolean;
};

export type Booking = {
  event_id: string;
  title: string;
  state: string;
  outcome: string;
  details: string;
  link: string;
  error: string;
  updated_at: string;
};

export const initialDraft: EventDraft = {
  club: "",
  title: "",
  date: "",
  summary: "",
  attendance: "20",
  campus: "St Lucia",
  building: "",
  room: "",
  roomSeats: "",
  coordinator: "",
  role: "",
  email: "",
  phone: "",
  setupStart: "17:30",
  setupEnd: "18:00",
  eventStart: "18:00",
  eventEnd: "20:00",
  packStart: "20:00",
  packEnd: "20:30",
  food: "",
  affiliated: "Yes",
  flexibility: "",
  under18: "0",
  nonUq: "0",
  indoor: true,
  seated: true,
  vehicles: false,
  structures: false,
  electrical: true,
  cash: false,
  alcohol: false,
  competition: false,
  offSite: false,
};

const quote = (value: string) => JSON.stringify(value.trim());
const yes = (value: boolean) => (value ? "true" : "false");

export function eventToml(draft: EventDraft, mode = "both", layout = "form") {
  const capacity = draft.roomSeats ? capacityBand(Number(draft.roomSeats)) : "";
  return `[output]
mode = ${quote(mode)}
risk_layout = ${quote(layout)}

[club]
name = ${quote(draft.club)}

[event]
title = ${quote(draft.title)}
date = ${quote(draft.date)}
campus = ${quote(draft.campus)}
building = ${quote(draft.building)}
room = ${quote(draft.room)}
summary = ${quote(draft.summary)}
estimated_attendance = ${quote(draft.attendance)}

[coordinator]
name = ${quote(draft.coordinator)}
role = ${quote(draft.role)}
email = ${quote(draft.email)}
phone = ${quote(draft.phone)}

[schedule]
setup_start = ${quote(draft.setupStart)}
setup_end = ${quote(draft.setupEnd)}
event_start = ${quote(draft.eventStart)}
event_end = ${quote(draft.eventEnd)}
pack_up_start = ${quote(draft.packStart)}
pack_up_end = ${quote(draft.packEnd)}

[catering]
provided = ${yes(Boolean(draft.food.trim()))}
description = ${quote(draft.food)}

[operations]
indoor = ${yes(draft.indoor)}
seated = ${yes(draft.seated)}
vehicles = ${yes(draft.vehicles)}
temporary_structures = ${yes(draft.structures)}
electrical_equipment = ${yes(draft.electrical)}
cash_handling = ${yes(draft.cash)}
alcohol = ${yes(draft.alcohol)}
physical_competitions = ${yes(draft.competition)}
off_site_impact = ${yes(draft.offSite)}

[form]
action = "preview"

[form.values]
affiliated = ${quote(draft.affiliated)}
date_flexibility = ${quote(draft.flexibility)}
under_18 = ${quote(draft.under18 || "0")}
non_uq = ${quote(draft.nonUq || "0")}
venue_capacity = ${quote(capacity)}
option_28 = "No"
food = ${quote(draft.food)}
option_30 = "No"
option_31 = "No"
option_32 = "No"
terms = "Agree"
`;
}

export function capacityBand(seats: number) {
  if (!Number.isFinite(seats) || seats <= 0) return "";
  if (seats <= 20) return "1-20";
  if (seats <= 50) return "21-50";
  if (seats <= 100) return "51-100";
  if (seats <= 200) return "101-200";
  return "201+";
}
