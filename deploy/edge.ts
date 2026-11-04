interface Assets {
  fetch(request: Request): Promise<Response>;
}

interface Environment {
  ASSETS: Assets;
}

const COOKIE = "__Host-soc_workspace";
const TOKEN = /^[a-f0-9]{64}$/;
const csp =
  "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; form-action 'self'; script-src 'self' 'sha256-Fhhc/juoTrt2h3OyahCtaTIAR86dKnTtGILNhIqZgIg='; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'";
const installer = `#!/bin/sh
set -eu

if command -v cargo >/dev/null 2>&1; then
  CARGO=cargo
elif [ -x "\${HOME}/.cargo/bin/cargo" ]; then
  CARGO="\${HOME}/.cargo/bin/cargo"
else
  echo "Installing Rust from the official rustup installer"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  CARGO="\${HOME}/.cargo/bin/cargo"
fi

"\${CARGO}" install --locked --force --git https://github.com/keys-i/soc.minis.git --rev f0f76539bdde9b2052bf311692bf77b03eb4e631 soc-minis
echo "Installed SOC Minis. Run: soc --help"
`;

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

function nativeOnly(): Response {
  return secured(
    Response.json(
      {
        error: "This action needs the SOC CLI on your computer. Install it from /soc/install",
        status: "native",
      },
      { status: 501 },
    ),
    true,
  );
}

function cookie(request: Request): string {
  for (const entry of (request.headers.get("Cookie") || "").split(";")) {
    const [name, value] = entry.trim().split("=", 2);
    if (name === COOKIE && TOKEN.test(value || "")) return value;
  }
  return "";
}

async function browserStatus(request: Request): Promise<Response> {
  const current = cookie(request);
  const token = current || Array.from(crypto.getRandomValues(new Uint8Array(32)), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token));
  const workspace = Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  const headers = current
    ? undefined
    : {
        "Set-Cookie": `${COOKIE}=${token}; Max-Age=31536000; Path=/; Secure; HttpOnly; SameSite=Lax`,
      };
  return secured(
    Response.json(
      {
        workspace,
        microsoft_mailbox: null,
        microsoft_ready: false,
        forms_ready: false,
      },
      { headers },
    ),
    true,
  );
}

function install(request: Request): Response {
  return secured(
    new Response(request.method === "HEAD" ? null : installer, {
      headers: {
        "Content-Type": "text/x-shellscript; charset=utf-8",
        "Content-Disposition": 'inline; filename="install-soc-minis.sh"',
        "Cache-Control": "public, max-age=300",
      },
    }),
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
    if (url.pathname === "/soc/install") {
      if (request.method !== "GET" && request.method !== "HEAD")
        return secured(new Response("Method not allowed", { status: 405 }), true);
      return install(request);
    }
    if (
      request.method === "GET" &&
      (url.pathname === "/soc/health" ||
        url.pathname === "/soc/health/live" ||
        url.pathname === "/soc/health/ready" ||
        url.pathname === "/soc/api/health" ||
        url.pathname === "/soc/api/health/live" ||
        url.pathname === "/soc/api/health/ready")
    )
      return secured(Response.json({ status: "ok", service: "soc-minis-edge" }), true);
    if (request.method === "GET" && url.pathname === "/soc/api/status")
      return browserStatus(request);
    if (
      url.pathname === "/soc/api" ||
      url.pathname.startsWith("/soc/api/") ||
      url.pathname === "/soc/health" ||
      url.pathname.startsWith("/soc/health/")
    )
      return nativeOnly();
    if (request.method !== "GET" && request.method !== "HEAD")
      return secured(new Response("Method not allowed", { status: 405 }), true);

    const pathname = url.pathname.slice("/soc".length) || "/";
    const name = pathname.split("/").pop() || "";
    let response = await environment.ASSETS.fetch(
      assetRequest(request, name.includes(".") ? pathname : "/index.html"),
    );
    if (!name.includes(".")) {
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
