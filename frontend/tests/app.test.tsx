import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { App, pageFromPath } from "../src/app";
import { status } from "../src/api";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, scrollTo: vi.fn() });

vi.mock("../src/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/api")>()),
  status: vi
    .fn()
    .mockResolvedValue({
      workspace: "0123456789abcdef01234567",
      microsoft_mailbox: null,
      microsoft_ready: false,
      forms_ready: false,
    }),
  bookings: vi.fn().mockResolvedValue([]),
}));

afterEach(() => {
  vi.unstubAllGlobals();
  history.replaceState(null, "", "/soc/");
  localStorage.clear();
  delete document.documentElement.dataset.theme;
  vi.mocked(status).mockResolvedValue({
    workspace: "0123456789abcdef01234567",
    microsoft_mailbox: null,
    microsoft_ready: false,
    forms_ready: false,
  });
});

describe("mini routes", () => {
  test("keeps the home focused on three real mini pages", () => {
    history.replaceState(null, "", "/soc/");
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(<App />);

    const links = [...host.querySelectorAll<HTMLAnchorElement>(".mini-landmark")];
    expect(links.map((link) => link.textContent)).toEqual([
      expect.stringContaining("Eventy"),
      expect.stringContaining("Monkey"),
      expect.stringContaining("Meddy"),
    ]);
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/soc/eventy",
      "/soc/monkey",
      "/soc/meddy",
    ]);
    expect(host.querySelector('.tectonic-footer [href="https://keysi.dev"]')).not.toBeNull();
    expect(
      host.querySelector('.tectonic-footer [href="https://linkedin.com/in/radhesh-g"]'),
    ).not.toBeNull();
    expect(host.querySelector(".eventy-workspace")).toBeNull();
  });

  test("keeps coming-soon minis honest on their own pages", async () => {
    history.replaceState(null, "", "/soc/monkey");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(host.querySelector(".preview-monkey")).not.toBeNull());

    expect(host.querySelector(".preview-monkey")).not.toBeNull();
    expect(host.textContent).toContain("Coming soon");
    expect(host.textContent).toContain("No bank or messaging account is connected yet");
    expect(host.querySelector("main button, main input, main a")).toBeNull();
    await act(async () => root.unmount());
    host.remove();
  });

  test("moves between mini pages without replacing the document", async () => {
    history.replaceState(null, "", "/soc/");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));

    const monkey = host.querySelector<HTMLAnchorElement>('[href="/soc/monkey"]')!;
    await act(async () => monkey.click());
    expect(location.pathname).toBe("/soc/monkey");
    await vi.waitFor(() => expect(host.querySelector(".preview-monkey")).not.toBeNull());

    await act(async () => root.unmount());
    host.remove();
  });

  test("toggles day and night directly while keeping system sync in a small menu", async () => {
    history.replaceState(null, "", "/soc/");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));

    const appearance = host.querySelector<HTMLButtonElement>('[aria-label="Night mode"]')!;
    expect(appearance.getAttribute("aria-pressed")).toBe("false");
    expect(host.querySelector('[popover="auto"]')?.textContent).toContain("Sync to system");
    expect(host.querySelector("select")).toBeNull();
    await act(async () => appearance.click());
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("soc.theme")).toBe("dark");
    expect(appearance.getAttribute("aria-pressed")).toBe("true");

    await act(async () => root.unmount());
    host.remove();
  });

  test("follows operating-system changes until a person chooses a theme", async () => {
    let dark = false,
      notify = () => {};
    vi.stubGlobal("matchMedia", (query: string) => ({
      get matches() {
        return dark;
      },
      addEventListener: (_type: string, listener: () => void) => {
        if (query.includes("color-scheme")) notify = listener;
      },
      removeEventListener: vi.fn(),
    }));
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));

    const appearance = host.querySelector<HTMLButtonElement>('[aria-label="Night mode"]')!;
    expect(localStorage.getItem("soc.theme")).toBe("system");
    expect(appearance.getAttribute("aria-pressed")).toBe("false");
    dark = true;
    await act(async () => notify());
    expect(appearance.getAttribute("aria-pressed")).toBe("true");

    await act(async () => root.unmount());
    host.remove();
  });

  test("switches the Eventy workspace and preserves its direct URL", async () => {
    history.replaceState(null, "", "/soc/eventy");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(host.querySelector(".eventy-tabs")).not.toBeNull());

    const tabs = [...host.querySelectorAll<HTMLButtonElement>(".eventy-tabs button")];
    expect(tabs).toHaveLength(2);
    await act(async () => tabs[1].click());
    expect(tabs[1].getAttribute("aria-pressed")).toBe("true");
    expect(host.querySelector(".eventy-workspace")?.getAttribute("aria-label")).toBe(
      "Booking follow-up",
    );
    expect(host.textContent).toContain("Booking mailbox");
    expect(location.pathname + location.search).toBe("/soc/eventy?view=follow");

    await act(async () => root.unmount());
    host.remove();
  });

  test("waits for the hosted workspace before opening a draft", async () => {
    let open!: (account: Awaited<ReturnType<typeof status>>) => void;
    vi.mocked(status).mockReturnValueOnce(
      new Promise((resolve) => {
        open = resolve;
      }),
    );
    history.replaceState(null, "", "/soc/eventy");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(host.querySelector(".workspace-loading")).not.toBeNull());

    expect(host.querySelector(".workspace-loading")?.textContent).toContain(
      "Opening your private workspace",
    );
    expect(host.querySelector('[name="club"]')).toBeNull();
    expect(localStorage.getItem("soc-minis:workspace:v1")).toBeNull();

    await act(async () =>
      open({
        workspace: "fedcba9876543210fedcba98",
        microsoft_mailbox: null,
        microsoft_ready: false,
        forms_ready: false,
      }),
    );
    await vi.waitFor(() => expect(host.querySelector('[name="club"]')).not.toBeNull());
    expect(localStorage.getItem("soc-minis:workspace:v1")).toBeNull();

    await act(async () => root.unmount());
    host.remove();
  });

  test("parses only supported mini routes", () => {
    expect(pageFromPath("/soc/meddy")).toBe("meddy");
    expect(pageFromPath("/soc/unknown")).toBe("not-found");
  });

  test("keeps an unknown SOC address inside a useful not-found page", async () => {
    history.replaceState(null, "", "/soc/missing-mini");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(host.querySelector(".lost-page")).not.toBeNull());

    expect(host.querySelector(".lost-page")).not.toBeNull();
    expect(host.textContent).toContain("That island isn't on the chart");
    expect(host.querySelector<HTMLAnchorElement>(".lost-copy a")?.getAttribute("href")).toBe(
      "/soc/",
    );
    await act(async () => root.unmount());
    host.remove();
  });

  test("restores the semantic draft and output choices", async () => {
    localStorage.setItem("soc-minis-event-v1", JSON.stringify({ club: "UQ Robotics" }));
    localStorage.setItem(
      "soc-minis-event-options-v1",
      JSON.stringify({ mode: "risk", layout: "pack" }),
    );
    history.replaceState(null, "", "/soc/eventy");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));
    await vi.waitFor(() => expect(host.querySelector('[name="club"]')).not.toBeNull());

    const club = host.querySelector<HTMLInputElement>('[name="club"]')!;
    expect(club.value).toBe("UQ Robotics");
    expect(club.autocomplete).toBe("organization");
    const prepare = [...host.querySelectorAll<HTMLButtonElement>(".steps button")].at(-1)!;
    await act(async () => prepare.click());
    expect(host.querySelector<HTMLInputElement>('[name="mode"][value="risk"]')?.checked).toBe(true);
    expect(host.querySelector<HTMLInputElement>('[name="layout"][value="pack"]')?.checked).toBe(
      true,
    );

    await act(async () => root.unmount());
    host.remove();
  });
});
