import assert from 'node:assert/strict';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { test } from 'node:test';
import { parseFrontmatter } from 'astro/markdown';
import { createAgentDocs, readChapters, type Chapter } from '../src/lib/agent-docs.ts';

test('the index and exports cover exactly the visible book, with summaries', async () => {
	const directory = new URL('../../docs/', import.meta.url);
	const files = (await readdir(directory, { recursive: true })).filter((file) => file.endsWith('.md'));
	const expectedPaths: string[] = [];
	const hiddenPaths: string[] = [];
	for (const file of files) {
		const { frontmatter } = parseFrontmatter(await readFile(new URL(file, directory), 'utf8'));
		const path = `/docs/${frontmatter.slug || 'index'}.md`;
		(frontmatter.sidebar?.hidden ? hiddenPaths : expectedPaths).push(path);
	}
	const chapters = await readChapters();
	const outputs = createAgentDocs(chapters);
	const index = outputs.get('/docs/llms.txt')!;
	const links = [...index.matchAll(/^- \[.*\]\(https:\/\/submilli\.ai([^)]*)\): (.+)$/gm)];
	assert.deepEqual(links.map((link) => link[1]).sort(), expectedPaths.sort());
	assert.deepEqual([...outputs.keys()].filter((path) => path.endsWith('.md')).sort(), expectedPaths);
	assert.equal(chapters[0].slug, '');
	assert.ok(chapters.findIndex((chapter) => chapter.slug === 'skill') <
		chapters.findIndex((chapter) => chapter.slug === 'the-language'));
	for (const path of expectedPaths) {
		const markdown = outputs.get(path)!;
		assert.match(markdown, /^# .+\n/);
		assert.ok(outputs.get('/docs/llms-full.txt')!.includes(markdown));
	}
	for (const path of hiddenPaths) {
		assert.ok(!outputs.has(path));
		assert.ok(!index.includes(`https://submilli.ai${path}`));
		assert.ok(!outputs.get('/docs/llms-full.txt')!.includes(`Source: https://submilli.ai${path}\n`));
	}
});

test('link rewriting preserves examples and callouts, resolves anchors, and respects hidden targets', () => {
	const code = '```ts title="example.ts"\nconst link = "[next](/docs/next)";\n```';
	const body = [
		'[next](/docs/next#section)', '[relative](next?example=1#section)',
		'[hidden](/docs/hidden)', '[local](#section)', '[root](/docs/)',
		'[reference][next]', '[next]: /docs/next "Next chapter"',
		'![image](/image.svg)', '[external](https://example.com)',
		'`[literal](/docs/next)`', ':::note\nKeep this callout.\n:::', code,
	].join('\n\n');
	const output = createAgentDocs([chapter('current', body), chapter('next'), chapter('')])
		.get('/docs/current.md')!;
	assert.ok(output.includes('(https://submilli.ai/docs/next.md#section)'));
	assert.ok(output.includes('(https://submilli.ai/docs/next.md?example=1#section)'));
	assert.ok(output.includes('(https://submilli.ai/docs/hidden)'));
	assert.ok(output.includes('(https://submilli.ai/docs/current.md#section)'));
	assert.ok(output.includes('(https://submilli.ai/docs/index.md)'));
	assert.ok(output.includes('[next]: https://submilli.ai/docs/next.md "Next chapter"'));
	assert.ok(output.includes('![image](https://submilli.ai/image.svg)'));
	assert.ok(output.includes('[external](https://example.com)'));
	assert.ok(output.includes('`[literal](/docs/next)`'));
	assert.ok(output.includes(':::note\nKeep this callout.\n:::'));
	assert.ok(output.includes(code));
});

test('hidden stubs need no description; malformed and colliding published slugs fail', async () => {
	const directory = await mkdtemp(join(tmpdir(), 'submilli-agent-docs-'));
	const url = pathToFileURL(`${directory}/`);
	try {
		await writeFile(join(directory, 'hidden.md'), '---\ntitle: Stub\nsidebar:\n  hidden: true\n---\nStub.');
		assert.deepEqual(await readChapters(url), []);
		await writeFile(join(directory, 'page.md'), '---\ntitle: Page\nslug: page\n---\nBody.');
		await assert.rejects(readChapters(url), /provide a description/);
		await writeFile(join(directory, 'page.md'), source('../escape'));
		await assert.rejects(readChapters(url), /URL-safe slug/);
		await writeFile(join(directory, 'page.md'), source(''));
		await writeFile(join(directory, 'duplicate.md'), source('index'));
		await assert.rejects(readChapters(url), /duplicate Markdown path/);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test('rewriting changes only destinations even with Unicode, tables, and linked images', () => {
	const body = [
		'😀 text [😀 next](/docs/next)',
		'| Link | Info |\n| --- | --- |\n| [a \\| b](/docs/next) | hello |\n| [filter](/docs/next?x=a\\|b) | query |',
		'[![img](/image.svg)](/docs/next)',
		'[label](</docs/next?x=1&y=2> "keep title")',
		'[parentheses](/asset\\(one\\).svg)',
	].join('\n\n');
	const output = createAgentDocs([chapter('current', body), chapter('next')]).get('/docs/current.md')!;
	assert.ok(output.endsWith(body
		.replaceAll('/docs/next', 'https://submilli.ai/docs/next.md')
		.replace('/image.svg', 'https://submilli.ai/image.svg')
		.replace('&y=2', '&amp;y=2')
		.replace('?x=a\\|b', '?x=a%7Cb')
		.replace('/asset\\(one\\).svg', 'https://submilli.ai/asset%28one%29.svg') + '\n'));
});

test('autolinks to visible chapters use Markdown while external URLs stay unchanged', () => {
	const body = '<https://submilli.ai/docs/next?x=1&y=2>\n\nhttps://submilli.ai/docs/next?x=1&y=2\n\n<https://example.com>';
	const output = createAgentDocs([chapter('current', body), chapter('next')]).get('/docs/current.md')!;
	assert.ok(output.endsWith(body.replaceAll('/docs/next', '/docs/next.md') + '\n'));
});

function chapter(slug: string, body = 'Chapter text.'): Chapter {
	return { file: `${slug}.md`, slug, title: slug || 'Home', description: 'Summary.', order: 1, body };
}

function source(slug: string): string {
	return `---\ntitle: Page\nslug: "${slug}"\ndescription: Summary\n---\nBody.`;
}
