import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { App } from "../src/app";

vi.mock("../src/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/api")>()),
  status: vi.fn().mockResolvedValue({ microsoft_mailbox: null, microsoft_ready: false, forms_ready: false }),
  bookings: vi.fn().mockResolvedValue([]),
}));

describe("product shelf", () => {
  test("keeps Monkey and Meddy as honest non-interactive previews", () => {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(<App />);

    const previews = [...host.querySelectorAll<HTMLElement>(".preview-product")];
    expect(previews).toHaveLength(2);
    expect(previews.map((preview) => preview.querySelector("h2")?.textContent)).toEqual(["Monkey", "Meddy"]);
    for (const preview of previews) {
      expect(preview.getAttribute("aria-disabled")).toBe("true");
      expect(preview.querySelector("a, button, input")).toBeNull();
      expect(preview.textContent).toContain("Coming soon");
    }
  });

  test("switches the real workspace when a route is selected", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<App />));

    const routes = [...host.querySelectorAll<HTMLAnchorElement>(".product-actions a")];
    expect(routes).toHaveLength(2);
    expect(routes.every((route) => route.getAttribute("href") === "#eventy-workspace")).toBe(true);

    await act(async () => routes[1].click());
    expect(routes[1].getAttribute("aria-current")).toBe("page");
    expect(host.querySelector("#eventy-workspace")?.getAttribute("aria-label")).toBe("Booking follow-up");
    expect(host.textContent).toContain("Booking mailbox");

    await act(async () => root.unmount());
    host.remove();
  });
});
