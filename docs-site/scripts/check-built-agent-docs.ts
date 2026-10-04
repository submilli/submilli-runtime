import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
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
assert.ok(!whyPage.includes('video-introduction'), 'An unavailable film does not create a chapter promo');
assert.ok(!whyPage.includes('/docs/docs/'), 'Sidebar links must not duplicate the docs base');
assert.ok(!whyPage.includes('<video'), 'Unpublished video must not create a production player');
const oldExecutionPage = await readFile(new URL('docs/concepts/execution-model/index.html', directory), 'utf8');
assert.ok(oldExecutionPage.includes('/docs/why/'), 'Former Concepts route redirects into the current book');
const embedPage = await readFile(new URL('docs/videos/embed/code-execution-introduction/index.html', directory), 'utf8');
assert.ok(embedPage.includes('Video unavailable.'), 'Unavailable embed has an honest fallback without production details');
assert.ok(embedPage.includes('noindex, nofollow'), 'Embeds stay out of search');
assert.ok(!embedPage.includes('<video') && !embedPage.includes('_video-preview'), 'Production embed never exposes local preview media');
const libraryPage = await readFile(new URL('docs/videos/index.html', directory), 'utf8');
assert.ok(!libraryPage.includes('class="transcript"') && !libraryPage.includes('Consider an agent used by a small business.'), 'Library keeps the full transcript on its deeper docs page');
const transcriptPage = await readFile(new URL('docs/videos/code-execution-introduction/index.html', directory), 'utf8');
assert.ok(transcriptPage.includes('Consider an agent used by a small business.'), 'Approved transcript remains available on its dedicated page');
assert.ok(!libraryPage.includes('Open the execution model') && !libraryPage.includes('<video'), 'Library needs no article detour or unpublished production player');
for (const page of [whyPage, embedPage, libraryPage, transcriptPage]) {
  assert.ok(!/Video publication pending|Revision pending|In review|pending recording|Read the complete transcript/.test(page), 'Public pages omit internal workflow and transcript promos');
}
assert.ok(!libraryPage.includes('data-video-id='), 'Unpublished films stay outside the public gallery');
assert.ok(!chapters.some((chapter) => chapter.slug.startsWith('next/')), 'Retired draft routes stay outside the book exports');
console.log(`Verified exports and HTML discovery for ${chapters.length} visible chapters.`);
