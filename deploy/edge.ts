interface Assets {
  fetch(request: Request): Promise<Response>;
}

interface Environment {
  ASSETS: Assets;
}

const csp =
  "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; form-action 'self'; script-src 'self' 'sha256-Fhhc/juoTrt2h3OyahCtaTIAR86dKnTtGILNhIqZgIg='; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'";

function secured(response: Response, privateResponse = false): Response {
  const headers = new Headers(response.headers);
  headers.set("Content-Security-Policy", csp);
  headers.set("Cross-Origin-Opener-Policy", "same-origin-allow-popups");
  headers.set("Cross-Origin-Resource-Policy", "same-origin");
  headers.set("Permissions-Policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=()");
  headers.set("Referrer-Policy", "no-referrer");
  headers.set("Strict-Transport-Security", "max-age=31536000");
  headers.set("X-Content-Type-Options", "nosniff");
  headers.set("X-Frame-Options", "DENY");
  if (privateResponse) headers.set("Cache-Control", "no-store");
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

function unavailable(): Response {
  return secured(
    Response.json(
      {
        error: "SOC Minis is online, but Eventy's private workspace service isn't available yet",
        status: "degraded",
      },
      { status: 503 },
    ),
    true,
  );
}

function assetRequest(request: Request, pathname: string): Request {
  const url = new URL(request.url);
  url.pathname = pathname;
  return new Request(url, request);
}

export default {
  async fetch(request: Request, environment: Environment): Promise<Response> {
    const url = new URL(request.url);
    if (url.hostname !== "uni.keysi.dev") return new Response("Not found", { status: 404 });
    if (url.pathname === "/soc") return secured(Response.redirect(new URL("/soc/", url), 308));
    if (!url.pathname.startsWith("/soc/")) return new Response("Not found", { status: 404 });
    if (
      url.pathname === "/soc/api" ||
      url.pathname.startsWith("/soc/api/") ||
      url.pathname === "/soc/health" ||
      url.pathname.startsWith("/soc/health/")
    )
      return unavailable();
    if (request.method !== "GET" && request.method !== "HEAD")
      return secured(new Response("Method not allowed", { status: 405 }), true);

    const pathname = url.pathname.slice("/soc".length) || "/";
    let response = await environment.ASSETS.fetch(assetRequest(request, pathname));
    const name = pathname.split("/").pop() || "";
    if (response.status === 404 && !name.includes(".")) {
      response = await environment.ASSETS.fetch(assetRequest(request, "/index.html"));
      const headers = new Headers(response.headers);
      headers.set("Cache-Control", "no-cache");
      response = new Response(response.body, {
        status: response.status,
        statusText: response.statusText,
        headers,
      });
    }
    return secured(response);
  },
};
