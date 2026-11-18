import { defineRouteMiddleware } from "@astrojs/starlight/route-data";

// /en/brukerveiledning/* and /en/teknisk/* are Norwegian fallbacks. Keep them out of
// search engines and out of the (English) search index; they exist so the sidebar links resolve.
export const onRequest = defineRouteMiddleware((context) => {
  const route = context.locals.starlightRoute;
  if (!route.isFallback) return;
  route.entry.data.pagefind = false;
  route.head.push({ tag: "meta", attrs: { name: "robots", content: "noindex" } });
});
