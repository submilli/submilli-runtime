import { defineRouteMiddleware } from '@astrojs/starlight/route-data';
import { films, videoHeadingId } from './lib/videos';

export const onRequest = defineRouteMiddleware(({ url, locals }) => {
  if (url.pathname.replace(/\/$/, '') !== '/docs/videos') return;

  // VideoLibrary renders these headings outside Markdown. Register them before
  // Starlight renders the desktop and mobile tables of contents.
  const route = locals.starlightRoute;
  const headings = films.map((film) => ({ depth: 2, slug: videoHeadingId(film), text: film.title }));
  route.headings.push(...headings);
  if (route.toc && route.toc.minHeadingLevel <= 2 && route.toc.maxHeadingLevel >= 2) {
    // This collection has no overview section, only the individual videos.
    route.toc.items = headings.map((heading) => ({ ...heading, children: [] }));
  }
});
