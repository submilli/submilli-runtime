import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { test } from 'node:test';
import { parseFrontmatter } from 'astro/markdown';
import { docTypeSchema } from '../src/lib/doc-type.ts';

test('documentation type requires an explicit Diataxis value', () => {
	for (const value of ['tutorial', 'how-to', 'explanation', 'reference']) {
		assert.equal(docTypeSchema.parse(value), value);
	}
	for (const value of [undefined, null, '', 'guide', 'Tutorial', 'tutorials', ['reference']]) {
		assert.equal(docTypeSchema.safeParse(value).success, false);
	}
});

test('every documentation page declares its purpose, including hidden and generated pages', async () => {
	const directory = new URL('../../docs/', import.meta.url);
	const files = (await readdir(directory, { recursive: true }))
		.filter((file) => file.endsWith('.md') && file !== 'WRITING.md');
	for (const file of files) {
		const { frontmatter } = parseFrontmatter(await readFile(new URL(file, directory), 'utf8'));
		assert.ok(docTypeSchema.safeParse(frontmatter.docType).success, `${file}: missing or invalid docType`);
	}
});
