import { isMini, miniById, type Mini } from "./minis";

export type Page = "home" | "not-found" | Mini;

export const basePath = import.meta.env.BASE_URL.replace(/\/$/, "") || "/soc";
export const homeHref = `${basePath}/`;

export function miniHref(mini: Mini) {
  return `${basePath}/${mini}`;
}

export function pageFromPath(pathname: string): Page {
  if (pathname !== basePath && !pathname.startsWith(`${basePath}/`)) return "home";
  const route = pathname.slice(basePath.length).replace(/^\/+|\/+$/g, "");
  if (!route) return "home";
  return isMini(route) ? route : "not-found";
}

export function ownsPath(pathname: string) {
  return pathname === basePath || pathname === homeHref || isMiniPath(pathname);
}

export function pageTitle(page: Page) {
  if (page === "home") return "SOC Minis";
  if (page === "not-found") return "Island not found | SOC Minis";
  return `${miniById[page].title} | SOC Minis`;
}

function isMiniPath(pathname: string) {
  return (Object.keys(miniById) as Mini[]).some((mini) => pathname === miniHref(mini));
}
