import assert from 'node:assert/strict';
import test from 'node:test';
import { toHtml } from 'hast-util-to-html';
import { ExpressiveCode, ExpressiveCodeBlock } from 'expressive-code';
import { classifyCodeBlock, codeBlocks } from '../src/plugins/code-blocks.mjs';

function metadata(source = '') {
	return new ExpressiveCodeBlock({ code: '', language: 'plaintext', meta: source }).metaOptions;
}

test('recognizes conservative bare command and output blocks', () => {
	assert.equal(classifyCodeBlock('submilli build init @acme/billing package', 'plaintext', metadata()), 'input');
	assert.equal(classifyCodeBlock('created .../quickstart/submilli.toml\ncreated .../quickstart/package/src/lib.ts', 'plaintext', metadata()), 'output');
});

test('honors explicit metadata and leaves unknown code neutral', () => {
	assert.equal(classifyCodeBlock('{"ok":true}', 'json', metadata('type="output"')), 'output');
	assert.equal(classifyCodeBlock('{"ok":true}', 'json', metadata()), undefined);
	assert.equal(classifyCodeBlock('const value = 1;', 'typescript', metadata()), undefined);
	assert.equal(classifyCodeBlock('export const value = 1;', 'typescript', metadata()), undefined);
});

test('keeps roles accessible without adding headers or changing the copy payload', async () => {
	const engine = new ExpressiveCode({ plugins: [codeBlocks()] });
	const { renderedGroupAst } = await engine.render({
		code: 'submilli build init @acme/billing package',
		language: 'plaintext',
	});
	const html = toHtml(renderedGroupAst);
	assert.match(html, /class="frame sub-code-input/);
	assert.match(html, /<span class="sub-code-role">Command<\/span>/);
	assert.match(html, /<figcaption class="header"><\/figcaption><span class="sub-code-role">Command<\/span><pre/);
	assert.match(html, /data-code="submilli build init @acme\/billing package"/);

	const titled = await engine.render({
		code: 'created ...\/quickstart\/submilli.toml',
		language: 'plaintext',
		meta: 'title="result"',
	});
	const titledHtml = toHtml(titled.renderedGroupAst);
	assert.match(titledHtml, /class="frame [^\"]*sub-code-output[^\"]*"/);
	assert.match(titledHtml, /sub-code-role">Output<\/span>/);
	assert.match(titledHtml, /title[^>]*>result<\/div>|title[^>]*>result<\/span>/);
});
