import test from 'node:test';
import assert from 'node:assert/strict';
import { access } from 'node:fs/promises';
import { films, introduction } from '../src/lib/videos.ts';
import registry from '../src/data/videos.json' with { type: 'json' };
import { readChapters } from '../src/lib/agent-docs.ts';

test('the independent series has stable embeds and book destinations', async () => {
  assert.deepEqual(films.map((film) => film.id), ['code-execution-introduction', 'challenges', 'helps', 'using', 'works', 'packages', 'blueprints']);
  assert.deepEqual(films.map((film) => film.id), registry.films.filter((film) => film.status === 'published').map((film) => film.id));
  for (const film of films) {
    assert.equal(film.status, 'published');
    assert.match(film.embedUrl, /^https:\/\/submilli-videos\.onrender\.com\/embed\/[a-z-]+\/$/);
    assert.ok(!/releases|\.mp4|\.vtt|publishedVersion|sha256/.test(JSON.stringify(film)));
    assert.ok(film.canonicalPath?.startsWith('/docs/'));
  }
  const packages = films.find((film) => film.id === 'packages');
  assert.equal(packages?.canonicalPath, '/docs/packages/');
  assert.equal(packages?.embedUrl, 'https://submilli-videos.onrender.com/embed/packages/');
  const blueprints = films.find((film) => film.id === 'blueprints');
  assert.equal(blueprints?.canonicalPath, '/docs/blueprints/');
  assert.equal(blueprints?.embedUrl, 'https://submilli-videos.onrender.com/embed/blueprints/');
  const works = films.find((film) => film.id === 'works');
  assert.equal(works?.canonicalPath, '/docs/server/#what-happens-to-a-program');
  assert.equal(works?.embedUrl, 'https://submilli-videos.onrender.com/embed/works/');
  const chapters = await readChapters();
  for (const film of films) assert.ok(chapters.some((chapter) => `/docs/${chapter.slug}/` === film.canonicalPath?.split('#')[0]), `${film.id} book destination`);
  assert.equal(introduction.transcriptPath, '/docs/videos/code-execution-introduction/');
  assert.equal(introduction.embedUrl, 'https://submilli-videos.onrender.com/embed/why-code/');
  await assert.rejects(access(new URL('../public/videos', import.meta.url)));
});
