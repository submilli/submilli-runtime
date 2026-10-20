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

test('agent prompts share ordered launch controls and an exact escaped payload', async () => {
  const prompt = 'Use a & b\nKeep "quotes", `code`, café, and </textarea><script>alert(1)</script>.';
  const engine = new ExpressiveCode({ plugins: [codeBlocks()] });
  const { renderedGroupAst } = await engine.render({code: prompt, language: 'text', meta: 'agent-prompt'});
  const html = toHtml(renderedGroupAst);
  const { selectAll } = await import('@expressive-code/core/hast');
  const links = selectAll('.agent-prompt-launch', renderedGroupAst);
  assert.deepEqual(links.map(link => link.properties.ariaLabel), ['Open in Codex', 'Open in Claude Code', 'Open in Cursor']);
  assert.deepEqual(links.map(link => {
    const url = new URL(link.properties.href);
    return url.searchParams.get('prompt') ?? url.searchParams.get('q') ?? url.searchParams.get('text');
  }), [prompt, prompt, prompt]);
  assert.equal(selectAll('textarea', renderedGroupAst)[0].children[0].value, prompt);
  assert.equal(selectAll('details', renderedGroupAst).length, 1);
  assert.equal(selectAll('script', renderedGroupAst).length, 0);
  assert.equal(selectAll('[data-copy-prompt]', renderedGroupAst).length, 1);
  assert.doesNotMatch(html, /class="copy"/); // no second Expressive Code copy button
});

test('ordinary text is not offered to agents and oversized prompts remain copyable', async () => {
  const engine = new ExpressiveCode({ plugins: [codeBlocks()] });
  const normal = await engine.render({code: 'A sample tool result', language: 'text'});
  assert.doesNotMatch(toHtml(normal.renderedGroupAst), /data-agent-prompt/);
  const long = await engine.render({code: 'é'.repeat(3000), language: 'text', meta: 'agent-prompt'});
  const { selectAll } = await import('@expressive-code/core/hast');
  assert.equal(selectAll('.agent-prompt-launch[href]', long.renderedGroupAst).length, 0);
  assert.equal(selectAll('[aria-disabled="true"]', long.renderedGroupAst).length, 3);
  assert.equal(selectAll('textarea', long.renderedGroupAst)[0].children[0].value, 'é'.repeat(3000));
});


test('demo asset URLs use the page origin without carrying page queries or changing other URLs', async () => {
  const { contextualizePrompt, agents, promptLink } = await import('../src/lib/agent-prompt-context.mjs');
  const base = 'https://submilli.ai/docs/examples/permission-demo/';
  const prompt = `Download ${base}blueprint.yaml\nand ${base}billing-source.zip\n\n- Keep https://submilli.ai/docs/install\n- Keep ${base}blueprint.yaml.backup\n- Keep ${base}blueprint.yaml?token=original\n- Keep café & <script> as text`;
  assert.equal(contextualizePrompt(prompt, 'https://submilli.ai/docs/how-to/x?private=value#secret'), prompt);
  assert.equal(contextualizePrompt(prompt, 'file:///tmp/docs/index.html'), prompt);
  for (const origin of ['http://127.0.0.1:4321', 'https://preview.example.test']) {
    const expected = prompt.replace(`${base}blueprint.yaml\n`, `${origin}/docs/examples/permission-demo/blueprint.yaml\n`)
      .replace(`${base}billing-source.zip\n`, `${origin}/docs/examples/permission-demo/billing-source.zip\n`);
    const actual = contextualizePrompt(prompt, `${origin}/docs/how-to/x?private=value#secret`);
    assert.equal(actual, expected);
    assert.ok(!actual.includes('private=value'));
    for (const agent of agents) {
      const url = new URL(promptLink(agent, actual));
      assert.equal(url.searchParams.get('prompt') ?? url.searchParams.get('q') ?? url.searchParams.get('text'), expected);
      assert.equal(promptLink(agent, actual.repeat(1000)), undefined);
    }
  }
});
