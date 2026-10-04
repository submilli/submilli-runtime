import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { films, introduction, challenges, videoSource } from '../src/lib/videos.ts';
import { createAgentDocs, readChapters, markdownPath } from '../src/lib/agent-docs.ts';

const directory = new URL('../dist/', import.meta.url);
const chapters = await readChapters();
for (const [path, expected] of createAgentDocs(chapters)) {
	assert.equal(await readFile(new URL(`.${path}`, directory), 'utf8'), expected, path);
}
for (const chapter of chapters) {
	const path = `docs/${chapter.slug ? `${chapter.slug}/` : ''}index.html`;
	const html = await readFile(new URL(path, directory), 'utf8');
	const markdown = markdownPath(chapter.slug);
	assert.ok(html.includes(`rel="alternate" type="text/markdown" href="https://submilli.ai${markdown}"`), path);
	assert.ok(html.includes('rel="describedby" href="https://submilli.ai/docs/llms.txt"'), path);
	assert.match(html, new RegExp(`href="${markdown.replace('.', '\\.')}"[^>]*>View Markdown</a>`), path);
	assert.match(html, /href="\/docs\/llms\.txt"[^>]*>Documentation for agents<\/a>/, path);
}
const files = await readdir(new URL('docs/', directory), { recursive: true });
const expectedMarkdown = chapters.map((chapter) => markdownPath(chapter.slug)).sort();
assert.deepEqual(files.filter((file) => file.endsWith('.md')).map((file) => `/docs/${file}`).sort(), expectedMarkdown);
for (const file of files.filter((file) => file.endsWith('.html'))) {
	const slug = file === 'index.html' ? '' : file.replace(/\/index\.html$/, '');
	if (chapters.some((chapter) => chapter.slug === slug)) continue;
	const html = await readFile(new URL(`docs/${file}`, directory), 'utf8');
	assert.ok(!html.includes('rel="alternate" type="text/markdown"'), file);
	assert.ok(!html.includes('>View Markdown</a>'), file);
}
const whyPage = await readFile(new URL('docs/why/index.html', directory), 'utf8');
for (const film of [introduction, challenges]) {
  assert.equal(whyPage.includes(`data-video-id="${film.id}"`), Boolean(videoSource(film)), `Chapter player follows ${film.id} publication state`);
}
assert.ok(!whyPage.includes('/docs/docs/'), 'Sidebar links must not duplicate the docs base');
assert.equal(whyPage.includes('<video'), [introduction, challenges].some(film => videoSource(film)), 'Chapter includes only available players');
const oldExecutionPage = await readFile(new URL('docs/concepts/execution-model/index.html', directory), 'utf8');
assert.ok(oldExecutionPage.includes('/docs/why/'), 'Former Concepts route redirects into the current book');
const embedPage = await readFile(new URL('docs/videos/embed/code-execution-introduction/index.html', directory), 'utf8');
assert.equal(embedPage.includes('Video unavailable.'), !videoSource(introduction), 'Embed fallback follows publication state');
assert.ok(embedPage.includes('noindex, nofollow'), 'Embeds stay out of search');
assert.ok(!embedPage.includes('_video-preview'), 'Production embed never exposes local preview media');
const libraryPage = await readFile(new URL('docs/videos/index.html', directory), 'utf8');
assert.ok(!libraryPage.includes('class="transcript"') && !libraryPage.includes('Consider an agent used by a small business.'), 'Library keeps the full transcript on its deeper docs page');
const transcriptPage = await readFile(new URL('docs/videos/code-execution-introduction/index.html', directory), 'utf8');
assert.ok(transcriptPage.includes('Consider an agent used by a small business.'), 'Approved transcript remains available on its dedicated page');
assert.ok(!libraryPage.includes('Open the execution model'), 'Library needs no article detour');
for (const page of [whyPage, embedPage, libraryPage, transcriptPage]) {
  assert.ok(!/Video publication pending|Revision pending|In review|pending recording|Read the complete transcript/.test(page), 'Public pages omit internal workflow and transcript promos');
}
let previousPosition = -1;
for (const film of films) {
  const position = libraryPage.indexOf(`data-video-id="${film.id}"`);
  const source = videoSource(film);
  assert.equal(position >= 0, Boolean(source), `${film.id} gallery visibility follows publication state`);
  const embed = await readFile(new URL(`docs/videos/embed/${film.id}/index.html`, directory), 'utf8');
  assert.equal(embed.includes('<video'), Boolean(source), `${film.id} embed visibility follows publication state`);
  assert.ok(!embed.includes('_video-preview'), `${film.id} embed cannot expose local media`);
  if (!source) continue;
  assert.ok(position > previousPosition, 'Available films preserve the series order');
  previousPosition = position;
  for (const page of [libraryPage, embed]) {
    assert.ok(page.includes(source), `${film.id} player uses its registered source`);
    assert.ok(page.includes('controls') && page.includes('playsinline'), `${film.id} has native controls`);
    assert.ok(film.captionsPath && page.includes(film.captionsPath), `${film.id} published captions are available`);
    assert.ok(!/<track[^>]*\sdefault(?:[\s=>])/.test(page), `${film.id} captions start disabled`);
  }
}
assert.ok(!chapters.some((chapter) => chapter.slug.startsWith('next/')), 'Retired draft routes stay outside the book exports');
console.log(`Verified exports and HTML discovery for ${chapters.length} visible chapters.`);
