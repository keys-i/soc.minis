import { lazy, Suspense, useEffect, useRef, useState, type MouseEvent } from "react";
import { flushSync } from "react-dom";
import { Footer } from "./footer";
import { Home } from "./home";
import { Mark } from "./habitat";
import { homeHref, ownsPath, pageFromPath, pageTitle } from "./route";

const Eventy = lazy(() => import("./eventy").then((module) => ({ default: module.Eventy })));
const Preview = lazy(() => import("./preview").then((module) => ({ default: module.Preview })));
const Lost = lazy(() => import("./lost").then((module) => ({ default: module.Lost })));

export { miniHref, pageFromPath } from "./route";
export type { Mini } from "./minis";

type Theme = "system" | "light" | "dark";

function savedTheme(): Theme {
  try {
    const theme = localStorage.getItem("soc.theme");
    if (theme === "light" || theme === "dark") return theme;
  } catch {
    // Browser storage is optional
  }
  return "system";
}

function applyTheme(theme: Theme) {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem("soc.theme", theme);
  } catch {
    // Browser storage is optional
  }
}

function systemDark() {
  return typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches;
}

function Appearance() {
  const [theme, setTheme] = useState(savedTheme),
    [dark, setDark] = useState(() => theme === "dark" || (theme === "system" && systemDark()));

  useEffect(() => {
    if (typeof matchMedia !== "function") {
      applyTheme(theme);
      setDark(theme === "dark");
      return;
    }
    const media = matchMedia("(prefers-color-scheme: dark)"),
      sync = () => setDark(theme === "system" ? media.matches : theme === "dark");
    applyTheme(theme);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, [theme]);

  return (
    <div className="theme-picker" data-dark={dark}>
      <button
        type="button"
        className="theme-toggle"
        aria-label="Night mode"
        aria-pressed={dark}
        onClick={() => setTheme(dark ? "light" : "dark")}
      >
        <span className="theme-track" aria-hidden="true">
          <span className="theme-knob">{dark ? "☾" : "☀"}</span>
        </span>
        <span>{dark ? "Night" : "Day"}</span>
      </button>
      <button
        type="button"
        className="theme-options"
        aria-label="Theme options"
        aria-haspopup="dialog"
        popoverTarget="soc-theme-menu"
      >
        <span aria-hidden="true">⌄</span>
      </button>
      <section
        id="soc-theme-menu"
        popover="auto"
        role="dialog"
        aria-label="Theme options"
        className="theme-menu"
      >
        <button
          type="button"
          className="quiet"
          aria-pressed={theme === "system"}
          popoverTarget="soc-theme-menu"
          popoverTargetAction="hide"
          onClick={() => setTheme("system")}
        >
          <span aria-hidden="true">{theme === "system" ? "✓" : ""}</span>
          Sync to system
        </button>
      </section>
    </div>
  );
}

export function App() {
  const [route, setRoute] = useState(() => location.pathname + location.search),
    page = pageFromPath(new URL(route, location.origin).pathname),
    mounted = useRef(false);

  useEffect(() => {
    document.title = pageTitle(page);
    if (mounted.current)
      document.querySelector<HTMLElement>("#main")?.focus({ preventScroll: true });
    else mounted.current = true;
  }, [page]);

  useEffect(() => {
    const back = () => setRoute(location.pathname + location.search);
    addEventListener("popstate", back);
    return () => removeEventListener("popstate", back);
  }, []);

  const navigate = (event: MouseEvent<HTMLDivElement>) => {
    const link = (event.target as HTMLElement).closest<HTMLAnchorElement>("a[href]");
    if (
      !link ||
      event.button ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey ||
      link.target ||
      link.download
    )
      return;
    const destination = new URL(link.href, location.href);
    if (destination.origin !== location.origin || !ownsPath(destination.pathname)) return;
    event.preventDefault();
    const commit = () => {
      history.pushState(null, "", destination);
      flushSync(() => setRoute(destination.pathname + destination.search));
      scrollTo({ top: 0 });
    };
    const transition = (document as TransitionDocument).startViewTransition;
    const reduceMotion =
      typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (transition && !reduceMotion) transition.call(document, commit);
    else commit();
  };

  return (
    <div className={`site-shell page-${page}`} onClick={navigate}>
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <header className="site-head">
        <a
          className="brand"
          href={homeHref}
          aria-label="SOC Minis home"
          aria-current={page === "home" ? "page" : undefined}
        >
          <span className="brand-mark">
            <Mark />
          </span>
          <span>SOC Minis</span>
        </a>
        <div className="head-actions">
          {page !== "home" && (
            <a className="all-minis" href={homeHref}>
              All minis
            </a>
          )}
          <Appearance />
        </div>
      </header>

      <Suspense
        fallback={
          <main id="main" className="route-loading" tabIndex={-1}>
            <span aria-hidden="true" />
            <p role="status">Sailing to {pageTitle(page)}…</p>
          </main>
        }
      >
        {page === "home" && <Home />}
        {page === "eventy" && <Eventy />}
        {page === "monkey" && <Preview mini="monkey" />}
        {page === "meddy" && <Preview mini="meddy" />}
        {page === "not-found" && <Lost />}
      </Suspense>

      <Footer />
    </div>
  );
}

type TransitionDocument = Document & {
  startViewTransition?: (update: () => void) => unknown;
};
