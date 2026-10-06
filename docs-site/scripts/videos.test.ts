import test from 'node:test';
import assert from 'node:assert/strict';
import { access } from 'node:fs/promises';
import { films, introduction } from '../src/lib/videos.ts';
import registry from '../src/data/videos.json' with { type: 'json' };
import { readChapters } from '../src/lib/agent-docs.ts';

test('the independent series has stable embeds and book destinations', async () => {
  assert.deepEqual(registry.films.map((film) => film.id), ['code-execution-introduction', 'challenges', 'helps', 'using', 'works']);
  assert.deepEqual(films.map((film) => film.id), registry.films.filter((film) => film.status === 'published').map((film) => film.id));
  for (const film of films) {
    assert.equal(film.status, 'published');
    assert.match(film.embedUrl, /^https:\/\/submilli-videos\.onrender\.com\/embed\/[a-z-]+\/$/);
    assert.ok(!/releases|\.mp4|\.vtt|publishedVersion|sha256/.test(JSON.stringify(film)));
    assert.ok(film.canonicalPath?.startsWith('/docs/'));
  }
  const chapters = await readChapters();
  for (const film of films) assert.ok(chapters.some((chapter) => `/docs/${chapter.slug}/` === film.canonicalPath?.split('#')[0]), `${film.id} book destination`);
  assert.equal(introduction.transcriptPath, '/docs/videos/code-execution-introduction/');
  assert.equal(introduction.embedUrl, 'https://submilli-videos.onrender.com/embed/why-code/');
  await assert.rejects(access(new URL('../public/videos', import.meta.url)));
});
