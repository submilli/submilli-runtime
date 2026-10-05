import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { test } from 'node:test';

const lychee = process.env.LYCHEE_BIN || 'lychee';

async function runLychee(root) {
	return new Promise((resolve, reject) => {
		const child = spawn(lychee, [
			'--offline', '--include-fragments=full', '--root-dir', root,
			'--remap', '^https://submilli\\.ai/docs([?#].*)?$ file://' + root + '/docs/$1',
			'--remap', '^https://submilli\\.ai/docs/(.*)$ file://' + root + '/docs/$1',
			'--index-files', 'index.html', join(root, 'index.html'),
		], { stdio: ['ignore', 'pipe', 'pipe'] });
		let output = '';
		child.stdout.on('data', (chunk) => { output += chunk; });
		child.stderr.on('data', (chunk) => { output += chunk; });
		child.once('error', reject);
		child.once('exit', (code, signal) => resolve({ code, signal, output }));
	});
}

async function fixture(body) {
	const root = await mkdtemp(join(tmpdir(), 'submilli-lychee-'));
	await mkdir(join(root, 'docs', 'page'), { recursive: true });
	await writeFile(join(root, 'docs', 'page', 'index.html'), '<h1 id="present">Page</h1>\n');
	await writeFile(join(root, 'docs', 'index.html'), '<h1 id="present">Home</h1>');
	await writeFile(join(root, 'index.html'), body);
	return root;
}

test('built-style local links and anchors pass', async (t) => {
	const root = await fixture([
		'<a href="/docs/page#present">Page</a>',
		'<a href="https://submilli.ai/docs/page#present">Canonical page</a>',
		'<a href="https://submilli.ai/docs?from=site#present">Canonical root</a>',
	].join('\n'));
	t.after(() => rm(root, { recursive: true, force: true }));
	const result = await runLychee(root);
	assert.equal(result.code, 0, result.output);
});

test('missing page fails', async (t) => {
	const root = await fixture('<a href="/docs/missing">Missing</a>');
	t.after(() => rm(root, { recursive: true, force: true }));
	const result = await runLychee(root);
	assert.equal(result.code, 2, result.output);
});

test('missing local anchor fails', async (t) => {
	const root = await fixture('<a href="/docs/page#missing">Missing anchor</a>');
	t.after(() => rm(root, { recursive: true, force: true }));
	const result = await runLychee(root);
	assert.equal(result.code, 2, result.output);
});

for (const url of ['https://submilli.ai/docs?from=site#missing', 'https://submilli.ai/docs/#missing', 'https://submilli.ai/docs/page?from=site#missing', 'https://submilli.ai/docs/missing']) {
	test(`broken canonical link fails: ${url}`, async (t) => {
		const root = await fixture(`<a href="${url}">Broken</a>`);
		t.after(() => rm(root, { recursive: true, force: true }));
		const result = await runLychee(root);
		assert.equal(result.code, 2, result.output);
	});
}
