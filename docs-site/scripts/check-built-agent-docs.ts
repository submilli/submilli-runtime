import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { films, introduction } from '../src/lib/videos.ts';
import videoRegistry from '../src/data/videos.json' with { type: 'json' };
import { createAgentDocs, readChapters, markdownPath } from '../src/lib/agent-docs.ts';

import { legacyDocsRoutes } from '../src/lib/legacy-docs.ts';

const directory = new URL('../dist/', import.meta.url);
const chapters = await readChapters();
for (const [path, expected] of createAgentDocs(chapters)) {
	assert.equal(await readFile(new URL(`.${path}`, directory), 'utf8'), expected, path);
}
for (const chapter of chapters) {
	const path = `docs/${chapter.slug ? `${chapter.slug}/` : ''}index.html`;
	const html = await readFile(new URL(path, directory), 'utf8');
	const markdown = markdownPath(chapter.slug);
	const jsonLd = [...html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map(match => JSON.parse(match[1]));
	assert.equal(jsonLd.length, 1, `${path}: one JSON-LD graph`);
	const pageSchema = jsonLd[0]['@graph'][0];
	assert.equal(pageSchema.url, `https://submilli.ai/docs/${chapter.slug ? `${chapter.slug}/` : ''}`, path);
	assert.equal(pageSchema.headline, chapter.title, path);
	assert.equal(pageSchema['@type'], chapter.slug ? 'TechArticle' : 'CollectionPage', path);
	assert.equal(jsonLd[0]['@graph'].length, chapter.slug ? 2 : 1, path);
	assert.ok(chapter.authorshipLabel, `${path}: missing confirmed authorship`);
	assert.ok(html.includes(`${chapter.authorshipLabel}. Authorship details`), `${path}: missing authorship icon`);
	assert.equal([...html.matchAll(/<button\b[^>]*\bdata-copy-page\b/g)].length, 1, `${path}: one page-copy control, including the docs home`);
	assert.ok(html.includes(`rel="alternate" type="text/markdown" href="https://submilli.ai${markdown}"`), path);
	assert.ok(html.includes('rel="describedby" href="https://submilli.ai/docs/llms.txt"'), path);
	assert.match(html, new RegExp(`href="${markdown.replace('.', '\\.')}"[^>]*>View Markdown</a>`), path);
	assert.match(html, /href="\/docs\/llms\.txt"[^>]*>Documentation for agents<\/a>/, path);
}
const files = await readdir(new URL('docs/', directory), { recursive: true });
const legacyMarkdown = Object.keys(legacyDocsRoutes).map(source => `/docs${source}.md`);
const expectedMarkdown = [...chapters.map(chapter => markdownPath(chapter.slug)), ...legacyMarkdown].sort();
for (const [source, target] of Object.entries(legacyDocsRoutes)) {
	const canonical = await readFile(new URL(`.${target.replace(/\/$/, '')}.md`, directory), 'utf8');
	assert.equal(await readFile(new URL(`./docs${source}.md`, directory), 'utf8'), canonical, source);
	const redirect = await readFile(new URL(`./docs${source}/index.html`, directory), 'utf8');
	assert.ok(redirect.includes('http-equiv="refresh"') && redirect.includes(target), source);
}
assert.deepEqual(files.filter((file) => file.endsWith('.md')).map((file) => `/docs/${file}`).sort(), expectedMarkdown);
for (const file of files.filter((file) => file.endsWith('.html'))) {
	const slug = file === 'index.html' ? '' : file.replace(/\/index\.html$/, '');
	if (chapters.some((chapter) => chapter.slug === slug)) continue;
	const html = await readFile(new URL(`docs/${file}`, directory), 'utf8');
	assert.ok(!html.includes('application/ld+json'), `${file}: no documentation schema on non-chapter pages`);
	assert.ok(!html.includes('rel="alternate" type="text/markdown"'), file);
	assert.ok(!html.includes('>View Markdown</a>'), file);
}
const whyPage = await readFile(new URL('docs/why/index.html', directory), 'utf8');
for (const film of films.filter((film) => film.canonicalPath?.split('#')[0] === '/docs/why/')) {
  assert.ok(whyPage.includes(`data-video-id="${film.id}"`));
  assert.ok(whyPage.includes(film.embedUrl));
}
for (const [id, slug, heading] of [['helps', 'why', 'what-submilli-is'], ['works', 'server', 'what-happens-to-a-program'], ['using', 'quickstart', '']] as const) {
  const film = films.find((film) => film.id === id);
  if (!film) continue;
  const page = await readFile(new URL(`docs/${slug}/index.html`, directory), 'utf8');
  assert.ok(page.includes(`data-video-id="${id}"`) && page.includes(film.embedUrl));
  if (heading) assert.ok(page.indexOf(`id="${heading}"`) < page.indexOf(`data-video-id="${id}"`));
}
const libraryPage = await readFile(new URL('docs/videos/index.html', directory), 'utf8');
let previousPosition = -1;
for (const film of films) {
  const position = libraryPage.indexOf(`data-video-id="${film.id}"`);
  assert.ok(position > previousPosition, `${film.id} preserves series order`);
  assert.ok(libraryPage.includes(film.embedUrl));
  previousPosition = position;
}
for (const film of films) {
  const legacyEmbed = await readFile(new URL(`docs/videos/embed/${film.id}/index.html`, directory), 'utf8');
  assert.ok(legacyEmbed.includes(`http-equiv="refresh"`) || legacyEmbed.includes(`http-equiv=\"refresh\"`), `${film.id} legacy embed forwards`);
  assert.ok(legacyEmbed.includes(film.embedUrl), `${film.id} legacy embed target`);
  assert.ok(!legacyEmbed.includes('<video') && !legacyEmbed.includes('/docs/videos/releases/'), `${film.id} legacy embed has no bundled media`);
}
const transcriptPage = await readFile(new URL('docs/videos/code-execution-introduction/index.html', directory), 'utf8');
assert.ok(transcriptPage.includes('https://submilli-videos.onrender.com/watch/why-code/'));
for (const file of await readdir(directory, { recursive: true })) {
  if (!file.endsWith('.html')) continue;
  const html = await readFile(new URL(file, directory), 'utf8');
  for (const film of videoRegistry.films.filter((film) => film.status !== 'published')) {
    assert.ok(!html.includes(film.embedUrl) && !html.includes(`data-video-id="${film.id}"`), `${file}: hidden film ${film.id}`);
  }
  assert.ok(!html.includes('<video'), file);
  assert.ok(!html.includes('/docs/videos/releases/'), file);
  assert.ok(!/publication pending|Revision pending|pending recording|Read the complete transcript/i.test(html), file);
}
for (const film of videoRegistry.films.filter((film) => film.status !== 'published')) {
  await assert.rejects(readFile(new URL(`docs/videos/embed/${film.id}/index.html`, directory)), { code: 'ENOENT' });
}
const oldExecutionPage = await readFile(new URL('docs/concepts/execution-model/index.html', directory), 'utf8');
assert.ok(oldExecutionPage.includes('/docs/why/'));
assert.equal(introduction.id, 'code-execution-introduction');
console.log(`Verified stable video embeds and HTML discovery for ${chapters.length} visible chapters.`);
