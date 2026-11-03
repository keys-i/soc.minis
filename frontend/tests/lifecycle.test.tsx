import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ErrorDialog } from "../src/dialog";
import { World, worldLayers } from "../src/world";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("archipelago lifecycle", () => {
  test("renders the six independently movable layers", () => {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(<World />);
    expect([...host.querySelectorAll(".world > span")].map((node) => node.className)).toEqual(
      worldLayers.map((layer) => `world-${layer}`),
    );
  });

  test("reads each destination camera from the mini manifest", () => {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(<World compact scene="eventy" />);
    const world = host.querySelector<HTMLElement>(".world")!;
    expect(world.dataset.scene).toBe("eventy");
    expect(world.style.getPropertyValue("--camera-x")).toBe("-18px");
    expect(world.style.getPropertyValue("--camera-y")).toBe("8px");
    expect(world.style.getPropertyValue("--camera-scale")).toBe("1.12");
  });

  test("pauses the animation loop while hidden or reduced motion is active", async () => {
    let hidden = false;
    const queries = new Map<string, FakeMedia>(),
      request = vi.fn(() => 7),
      cancel = vi.fn();
    vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
    vi.stubGlobal("requestAnimationFrame", request);
    vi.stubGlobal("cancelAnimationFrame", cancel);
    vi.stubGlobal("matchMedia", (query: string) => {
      const value = queries.get(query) || fakeMedia(query, query === "(pointer: fine)");
      queries.set(query, value);
      return value;
    });

    const host = document.createElement("div"),
      root = createRoot(host);
    document.body.append(host);
    await act(async () => root.render(<section><World /></section>));
    await vi.waitFor(() => expect(request).toHaveBeenCalledOnce());

    hidden = true;
    document.dispatchEvent(new Event("visibilitychange"));
    expect(cancel).toHaveBeenCalledWith(7);

    hidden = false;
    document.dispatchEvent(new Event("visibilitychange"));
    expect(request).toHaveBeenCalledTimes(2);

    const motion = queries.get("(prefers-reduced-motion: reduce)")!;
    motion.matches = true;
    motion.change();
    expect(cancel).toHaveBeenLastCalledWith(7);

    await act(async () => root.unmount());
    host.remove();
  });

  test("pauses off-screen and resumes when the world returns", async () => {
    let observe!: (entries: { isIntersecting: boolean }[]) => void;
    const request = vi.fn(() => 11),
      cancel = vi.fn(),
      disconnect = vi.fn();
    vi.stubGlobal("requestAnimationFrame", request);
    vi.stubGlobal("cancelAnimationFrame", cancel);
    vi.stubGlobal("matchMedia", (query: string) => fakeMedia(query, query === "(pointer: fine)"));
    vi.stubGlobal("IntersectionObserver", class {
      constructor(callback: typeof observe) { observe = callback; }
      observe() {}
      disconnect() { disconnect(); }
    });

    const host = document.createElement("div"),
      root = createRoot(host);
    document.body.append(host);
    await act(async () => root.render(<section><World /></section>));
    await vi.waitFor(() => expect(request).toHaveBeenCalledOnce());

    observe([{ isIntersecting: false }]);
    expect(cancel).toHaveBeenCalledWith(11);
    observe([{ isIntersecting: true }]);
    expect(request).toHaveBeenCalledTimes(2);

    await act(async () => root.unmount());
    expect(disconnect).toHaveBeenCalledOnce();
    host.remove();
  });

  test("keeps the world still for reduced-data and save-data preferences", async () => {
    const queries = new Map<string, FakeMedia>(),
      request = vi.fn(() => 13);
    let connectionChange = () => {};
    const connection = {
      saveData: true,
      addEventListener: (_type: string, listener: () => void) => { connectionChange = listener; },
      removeEventListener: vi.fn(),
    };
    Object.defineProperty(navigator, "connection", { configurable: true, value: connection });
    vi.stubGlobal("requestAnimationFrame", request);
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    vi.stubGlobal("matchMedia", (query: string) => {
      const value = queries.get(query) || fakeMedia(query, query === "(prefers-reduced-data: reduce)" || query === "(pointer: fine)");
      queries.set(query, value);
      return value;
    });

    const host = document.createElement("div"),
      root = createRoot(host);
    document.body.append(host);
    await act(async () => root.render(<section><World /></section>));
    expect(request).not.toHaveBeenCalled();
    await vi.waitFor(() =>
      expect(host.querySelector<HTMLElement>(".world")?.dataset.depth).toBe("still"),
    );

    queries.get("(prefers-reduced-data: reduce)")!.matches = false;
    queries.get("(prefers-reduced-data: reduce)")!.change();
    expect(request).not.toHaveBeenCalled();
    connection.saveData = false;
    connectionChange();
    expect(request).toHaveBeenCalledOnce();

    await act(async () => root.unmount());
    host.remove();
    delete (navigator as Navigator & { connection?: unknown }).connection;
  });

  test("hands coarse pointers to the CSS scroll-depth path", async () => {
    const request = vi.fn(() => 17);
    vi.stubGlobal("requestAnimationFrame", request);
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    vi.stubGlobal("matchMedia", (query: string) => fakeMedia(query, false));

    const host = document.createElement("div"),
      root = createRoot(host);
    document.body.append(host);
    await act(async () => root.render(<section><World /></section>));

    await vi.waitFor(() =>
      expect(host.querySelector<HTMLElement>(".world")?.dataset.depth).toBe("scroll"),
    );
    expect(request).not.toHaveBeenCalled();

    await act(async () => root.unmount());
    host.remove();
  });
});

describe("error dialog", () => {
  test("announces the problem and closes from Escape", async () => {
    const close = vi.fn(),
      host = document.createElement("div"),
      root = createRoot(host);
    document.body.append(host);
    await act(async () => root.render(<ErrorDialog message="The room search is unavailable" close={close} />));

    expect(host.querySelector('[role="alertdialog"]')?.getAttribute("aria-modal")).toBe("true");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(close).toHaveBeenCalledOnce();

    await act(async () => root.unmount());
    host.remove();
  });
});

type FakeMedia = MediaQueryList & { matches: boolean; change: () => void };

function fakeMedia(media: string, initial: boolean): FakeMedia {
  let listener: ((event: MediaQueryListEvent) => void) | undefined;
  return {
    matches: initial,
    media,
    onchange: null,
    addEventListener: (_type: string, callback: EventListenerOrEventListenerObject) => {
      listener = callback as (event: MediaQueryListEvent) => void;
    },
    removeEventListener: () => { listener = undefined; },
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
    change() { listener?.({ matches: this.matches } as MediaQueryListEvent); },
  };
}
