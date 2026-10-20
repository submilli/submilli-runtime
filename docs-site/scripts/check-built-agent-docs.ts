import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { films, introduction } from '../src/lib/videos.ts';
import videoRegistry from '../src/data/videos.json' with { type: 'json' };
import { createAgentDocs, readChapters, markdownPath } from '../src/lib/agent-docs.ts';

import { legacyDocsRoutes, legacyRedirectRoutes } from '../src/lib/legacy-docs.ts';

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
for (const [id, slug, heading] of [['helps', 'why', 'what-submilli-is'], ['using', 'quickstart', ''], ['works', 'server', 'what-happens-to-a-program'], ['packages', 'packages', ''], ['blueprints', 'blueprints', '']] as const) {
  const film = films.find((film) => film.id === id);
  assert.ok(film, `${id} is finalized and registered`);
  const page = await readFile(new URL(`docs/${slug}/index.html`, directory), 'utf8');
  assert.ok(page.includes(`data-video-id="${id}"`) && page.includes(film.embedUrl));
  if (heading) assert.ok(page.indexOf(`id="${heading}"`) < page.indexOf(`data-video-id="${id}"`));
}
const libraryPage = await readFile(new URL('docs/videos/index.html', directory), 'utf8');
// Component-rendered headings must be present in both server-rendered menus.
for (const tag of ['starlight-toc', 'mobile-starlight-toc']) {
  const navigation = libraryPage.match(new RegExp(`<${tag}\\b[^>]*>([\\s\\S]*?)</${tag}>`))?.[1];
  assert.ok(navigation, `${tag}: video navigation exists`);
  assert.ok(!navigation.includes('href="#_top"'), `${tag}: no artificial overview entry`);
  assert.deepEqual(
    [...navigation.matchAll(/href="#(video-[^"]+)"/g)].map((match) => match[1]),
    films.map((film) => `video-${film.id}-title`),
    `${tag}: every published video appears once in series order`,
  );
  const whyNavigation = whyPage.match(new RegExp(`<${tag}\\b[^>]*>([\\s\\S]*?)</${tag}>`))?.[1];
  assert.ok(whyNavigation && !whyNavigation.includes('href="#video-'), `${tag}: other pages keep their own navigation`);
}
const libraryContent = libraryPage.match(/<div[^>]*class="[^"]*sl-markdown-content[^>]*>([\s\S]*?)<\/main>/)?.[1];
assert.ok(libraryContent && !libraryContent.includes('Videos about code execution'), 'library has no repeated introduction');
let previousPosition = -1;
for (const film of films) {
  assert.equal([...libraryPage.matchAll(new RegExp(`<h2\\b[^>]*id="video-${film.id}-title"`, 'g'))].length, 1, `${film.id}: one navigable heading`);
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
const retiredTranscriptPage = await readFile(new URL('docs/videos/code-execution-introduction/index.html', directory), 'utf8');
assert.ok(retiredTranscriptPage.includes('http-equiv="refresh"') || retiredTranscriptPage.includes('http-equiv=\"refresh\"'));
assert.ok(retiredTranscriptPage.includes('/docs/why/#video-code-execution-introduction'));
assert.ok(retiredTranscriptPage.includes('name="robots" content="noindex"'));
assert.ok(!retiredTranscriptPage.includes('complete narration'));
assert.deepEqual(Object.keys(legacyRedirectRoutes), ['/videos/code-execution-introduction']);
const sitemap = await readFile(new URL('docs/sitemap-0.xml', directory), 'utf8');
assert.ok(!sitemap.includes('/docs/videos/code-execution-introduction'), 'retired transcript is absent from sitemap');
console.log(`Verified stable video embeds and HTML discovery for ${chapters.length} visible chapters.`);

for (const file of ['agent.py', 'requirements.txt']) {
	assert.equal(
		await readFile(new URL(`docs/examples/quickstart/${file}`, directory), 'utf8'),
		await readFile(new URL(`../../examples/quickstart/${file}`, import.meta.url), 'utf8'),
		`Quickstart download matches the verified example: ${file}`,
	);
}

// Downloads are native files, never generated HTML pages or SPA fallbacks.
for (const file of ['basic-blueprint.yaml', 'blueprint.yaml']) {
  assert.equal(
    await readFile(new URL(`docs/examples/permission-demo/${file}`, directory), 'utf8'),
    await readFile(new URL(`../../examples/harnesses/permission-demo/${file}`, import.meta.url), 'utf8'),
  );
}
const { execFileSync } = await import('node:child_process');
const { fileURLToPath } = await import('node:url');
const demoArchive = fileURLToPath(new URL('docs/examples/permission-demo/billing-source.zip', directory));
for (const file of ['submilli.toml', 'package/src/lib.ts', 'package/docs/readme.md']) {
  assert.equal(execFileSync('unzip', ['-p', demoArchive, file], { encoding: 'utf8' }),
    await readFile(new URL(`../../examples/quickstart/${file}`, import.meta.url), 'utf8'),
    `Permission demo download matches verified Package: ${file}`);
}
for (const slug of ['connect-mastra', 'connect-deepagents', 'connect-openai-agents', 'connect-claude-agent-sdk', 'use-the-http-api']) {
  for (const suffix of ['/index.html', '.md']) {
    const content = await readFile(new URL(`docs/how-to/${slug}${suffix}`, directory), 'utf8');
    for (const file of ['basic-blueprint.yaml', 'blueprint.yaml', 'billing-source.zip']) {
      assert.ok(content.includes(`/docs/examples/permission-demo/${file}`), `${slug}${suffix} links to ${file}`);
    }
  }
}
