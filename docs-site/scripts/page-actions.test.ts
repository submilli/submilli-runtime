import assert from 'node:assert/strict';
import test from 'node:test';
import { copyText, fetchMarkdown } from '../src/lib/page-actions.ts';

function response(body: string, status: number, contentType: string): Response {
	return new Response(body, { status, headers: { 'content-type': contentType } });
}

test('fetchMarkdown accepts a Markdown response and sends same-origin headers', async () => {
	let request: { url: string; init: RequestInit } | undefined;
	const markdown = await fetchMarkdown(async (url, init) => {
		request = { url, init };
		return response('# Page', 200, 'text/markdown; charset=utf-8');
	}, '/docs/page.md', new AbortController().signal);
	assert.equal(markdown, '# Page');
	assert.equal(request?.url, '/docs/page.md');
	assert.equal(request?.init.credentials, 'same-origin');
	assert.deepEqual(request?.init.headers, { Accept: 'text/markdown' });
});

test('fetchMarkdown accepts static-host plaintext Markdown', async () => {
	const markdown = await fetchMarkdown(async () => response('# Page', 200, 'text/plain; charset=utf-8'), '/docs/page.md', new AbortController().signal);
	assert.equal(markdown, '# Page');
});

test('fetchMarkdown rejects failed and non-Markdown responses', async () => {
	await assert.rejects(
		fetchMarkdown(async () => response('<html>error</html>', 500, 'text/html'), '/docs/page.md', new AbortController().signal),
		/Markdown request failed/,
	);
	await assert.rejects(
		fetchMarkdown(async () => response('{"ok":true}', 200, 'application/json'), '/docs/page.md', new AbortController().signal),
		/not Markdown/,
	);
});

test('copyText falls back to an explicit manual mode when clipboard copying fails', async () => {
	let removed = false;
	let restored = false;
	const textarea = {
		value: '', readOnly: false, style: { position: '', opacity: '' },
		setAttribute() {}, focus() {}, select() {}, remove() { removed = true; },
	};
	const method = await copyText('## Page', {
		navigator: { clipboard: { writeText: async () => { throw new Error('blocked'); } } },
		document: {
			body: { append() {} },
			activeElement: { focus() { restored = true; } },
			createElement: () => textarea,
			execCommand: () => { throw new Error('blocked'); },
		},
	});
	assert.equal(method, 'manual');
	assert.equal(textarea.value, '## Page');
	assert.equal(removed, true);
	assert.equal(restored, true);
});

test('copyText tolerates an active element without a focus method', async () => {
	const textarea = {
		value: '', readOnly: false, style: { position: '', opacity: '' },
		setAttribute() {}, focus() {}, select() {}, remove() {},
	};
	const method = await copyText('## Page', {
		navigator: {},
		document: {
			body: { append() {} },
			activeElement: {},
			createElement: () => textarea,
			execCommand: () => false,
		},
	});
	assert.equal(method, 'manual');
});
