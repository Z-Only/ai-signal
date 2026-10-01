import {
  createApi,
  type Core,
  type Dependencies,
  type Env,
  type Source,
  type Article,
} from "./adapter";
export interface RustBindings {
  public_sources_json: () => string;
  normalize_feed_json: (id: string, xml: string, now: string) => string;
}
export interface Asset {
  body: string;
  type: string;
}
export function createRustCore(bindings: RustBindings): Core {
  return {
    sources: JSON.parse(bindings.public_sources_json()) as Source[],
    parse(id, xml, now) {
      const result = JSON.parse(bindings.normalize_feed_json(id, xml, now)) as {
        ok: boolean;
        error?: string;
        articles?: Article[];
      };
      if (!result.ok || !Array.isArray(result.articles))
        throw Error(result.error ?? "Invalid Rust response");
      return result.articles;
    },
  };
}
export function createSite(
  core: Core,
  assets: Record<string, Asset>,
  deps: Dependencies = {},
) {
  const api = createApi(core, deps);
  return {
    async fetch(req: Request, env: Env): Promise<Response> {
      const url = new URL(req.url);
      if (url.pathname.startsWith("/api/") || url.pathname === "/healthz")
        return api(req, env);
      if (!["GET", "HEAD"].includes(req.method))
        return new Response("Method not allowed", { status: 405 });
      const path = url.pathname === "/" ? "/index.html" : url.pathname;
      const asset =
        assets[path] ??
        (!url.pathname.includes(".") ? assets["/index.html"] : undefined);
      if (!asset) return new Response("Not found", { status: 404 });
      return new Response(req.method === "HEAD" ? null : asset.body, {
        headers: {
          "Content-Type": asset.type,
          "Cache-Control": url.pathname.startsWith("/assets/")
            ? "public,max-age=31536000,immutable"
            : "no-cache",
          "X-Content-Type-Options": "nosniff",
          "Referrer-Policy": "strict-origin-when-cross-origin",
          "Content-Security-Policy":
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'self'; object-src 'none'",
        },
      });
    },
  };
}
