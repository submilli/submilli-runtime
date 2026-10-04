// Rewrites the generated regions of the reference pages from the binaries'
// own output, so the reference can't drift from the code. A page marks a
// region with `<!-- generated:NAME -->` and `<!-- /generated:NAME -->`; the
// text between them is replaced. Run after building the CLI and the server:
//
//   SUBMILLI_BIN=../target/release/submilli \
//   SUBMILLI_SERVER_BIN=../target/release/submilli-server npm run reference
//
// Both default to the binaries on PATH. The commands run in an empty
// SUBMILLI_HOME, so packages installed on this machine don't appear.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import ts from 'typescript';

const cli = process.env.SUBMILLI_BIN ?? 'submilli';
const server = process.env.SUBMILLI_SERVER_BIN ?? 'submilli-server';
const docs = new URL('../../docs/', import.meta.url);
const home = mkdtempSync(join(tmpdir(), 'submilli-reference-'));

function run(binary: string, args: string[]): string {
	return execFileSync(binary, args, {
		cwd: home,
		env: { ...process.env, SUBMILLI_HOME: join(home, 'home'), NO_COLOR: '1' },
		encoding: 'utf8',
		stdio: ['ignore', 'pipe', 'pipe'],
	}).trimEnd();
}

// ── CLI ──────────────────────────────────────────────────────────────────

/** The subcommand names listed under a help text's `Commands:` heading. */
function subcommands(help: string): string[] {
	const names: string[] = [];
	let inCommands = false;
	for (const line of help.split('\n')) {
		if (/^Commands:/.test(line)) inCommands = true;
		else if (inCommands && /^\S/.test(line)) inCommands = false;
		else if (inCommands) {
			const name = line.match(/^ {2}(\S+)/)?.[1];
			if (name && name !== 'help') names.push(name);
		}
	}
	return names;
}

function commandTree(binary: string, path: string[], depth: number, into: string[]): void {
	const help = run(binary, [...path, '--help']);
	const name = [binary === cli ? 'submilli' : 'submilli-server', ...path].join(' ');
	if (depth > 0) {
		into.push(`${'#'.repeat(Math.min(depth + 1, 4))} \`${name}\``, '', '```text', help, '```', '');
	}
	for (const sub of subcommands(help)) commandTree(binary, [...path, sub], depth + 1, into);
}

function cliReference(): string {
	const out: string[] = [];
	commandTree(cli, [], 0, out);
	return out.join('\n');
}

function serverReference(): string {
	return ['```text', run(server, ['--help']), '```', ''].join('\n');
}

// ── Capabilities ─────────────────────────────────────────────────────────

function capabilities(): string {
	const listing = run(cli, ['blueprint', 'capability', 'list']);
	const out: string[] = [];
	let row: { name: string; description: string; fields: string; example: string } | null = null;
	const flush = () => {
		if (!row) return;
		out.push(`| \`${row.name}\` | ${cell(row.fields)} | ${cell(row.description)} | ${row.example ? code(row.example) : ''} |`);
		row = null;
	};
	for (const line of listing.split('\n')) {
		const module = line.match(/^(\S+)$/)?.[1];
		const capability = line.match(/^ {2}(\S+) — (.*)$/);
		const fields = line.match(/^ {6}fields: (.*)$/)?.[1];
		const example = line.match(/^ {6}example filter: (.*)$/)?.[1];
		if (module) {
			flush();
			out.push('', `### \`${module}\``, '', '| Capability | Fields | Operation | Example filter |', '| --- | --- | --- | --- |');
		} else if (capability) {
			flush();
			row = { name: capability[1], description: capability[2], fields: '', example: '' };
		} else if (fields !== undefined && row) {
			row.fields = fields.split(', ').map((field) => code(field)).join(', ');
		} else if (example !== undefined && row) {
			row.example = example;
		}
	}
	flush();
	return out.join('\n').trim() + '\n';
}

// ── Declarations ─────────────────────────────────────────────────────────

interface Doc {
	summary: string;
	capabilities: string[];
}

function docOf(node: ts.Node): Doc {
	const comments = ts.getJSDocCommentsAndTags(node).filter(ts.isJSDoc);
	const jsDoc = comments.at(-1);
	const text = ts.getTextOfJSDocComment(jsDoc?.comment) ?? '';
	const capabilities = (jsDoc?.tags ?? [])
		.filter((tag) => tag.tagName.text === 'capability')
		.map((tag) => (ts.getTextOfJSDocComment(tag.comment) ?? '').trim());
	return { summary: firstSentence(text), capabilities };
}

function firstSentence(text: string): string {
	const flat = text.replace(/\s+/g, ' ').trim();
	// A sentence ends at `.`, `!` or `?` before a space, except after an
	// abbreviation such as "e.g." or "i.e.".
	const end = /(?<!\b(?:e\.g|i\.e|etc|vs))[.!?](?=\s|$)/.exec(flat);
	return end ? flat.slice(0, end.index + 1) : flat;
}

/** The declaration's own text, without its doc comment or a body. */
function signature(node: ts.Node, source: ts.SourceFile): string {
	return node.getText(source).replace(/\s+/g, ' ').replace(/;$/, '').replaceAll(KEYWORD, '');
}

function memberTable(members: readonly ts.Node[], source: ts.SourceFile): string[] {
	if (!members.length) return [];
	const docs = members.map((member) => docOf(member));
	// A capability column only where some member is gated, as on `Repository`.
	if (docs.some((doc) => doc.capabilities.length)) {
		return ['| Member | Capability | Description |', '| --- | --- | --- |',
			...members.map((member, i) =>
				`| ${code(signature(member, source))} | ${docs[i].capabilities.map((c) => code(c)).join('<br>')} | ${cell(docs[i].summary)} |`), ''];
	}
	return ['| Member | Description |', '| --- | --- |',
		...members.map((member, i) => `| ${code(signature(member, source))} | ${cell(docs[i].summary)} |`), ''];
}

// `function delete(…)` is a valid stdlib export but a TypeScript keyword, so
// keyword names are parsed under a placeholder and restored in the output.
const KEYWORD = '__keyword_';
function parseable(text: string): string {
	return text.replace(/^function (delete|default|new|in|import|export)\(/gm, `function ${KEYWORD}$1(`);
}

function describeDeclarations(text: string, level: number, prefix = ''): string[] {
	const source = ts.createSourceFile('declarations.d.ts', parseable(text), ts.ScriptTarget.Latest, true);
	return describeStatements(source.statements, source, level, prefix);
}

function describeStatements(
	statements: readonly ts.Statement[],
	source: ts.SourceFile,
	level: number,
	prefix: string,
): string[] {
	const out: string[] = [];
	const functions = statements.filter(ts.isFunctionDeclaration);
	if (functions.length) {
		out.push('| Function | Capability | Description |', '| --- | --- | --- |');
		for (const fn of functions) {
			const doc = docOf(fn);
			const gates = doc.capabilities.map((capability) => code(capability)).join('<br>');
			out.push(`| ${code(signature(fn, source).replace(/^function /, ''))} | ${gates} | ${cell(doc.summary)} |`);
		}
		out.push('');
	}
	const constants = statements.filter(ts.isVariableStatement);
	if (constants.length) {
		out.push('| Constant | Description |', '| --- | --- |');
		for (const statement of constants) {
			out.push(`| ${code(signature(statement.declarationList, source).replace(/^const /, ''))} | ${cell(docOf(statement).summary)} |`);
		}
		out.push('');
	}
	for (const statement of statements) {
		const heading = '#'.repeat(Math.min(level, 6));
		if (ts.isInterfaceDeclaration(statement) || ts.isClassDeclaration(statement)) {
			const name = `${prefix}${statement.name?.text ?? ''}`;
			out.push(`${heading} \`${name}\``, '');
			const doc = docOf(statement);
			if (doc.summary) out.push(doc.summary, '');
			out.push(...memberTable(statement.members, source));
		} else if (ts.isTypeAliasDeclaration(statement)) {
			out.push(`${heading} \`${prefix}${statement.name.text}\``, '');
			const doc = docOf(statement);
			if (doc.summary) out.push(doc.summary, '');
			out.push('```typescript', statement.getText(source), '```', '');
		} else if (ts.isModuleDeclaration(statement) && statement.body && ts.isModuleBlock(statement.body)) {
			const name = `${prefix}${statement.name.getText(source)}`;
			out.push(...describeStatements(statement.body.statements, source, level, `${name}.`));
		}
	}
	return out;
}

function standardLibrary(): string {
	const modules = run(cli, ['search', ''])
		.split('\n')
		.map((line) => line.match(/^(submilli:[a-z]+) — (.*)$/))
		.filter((match): match is RegExpMatchArray => match !== null);
	const out: string[] = [];
	for (const [, name, description] of modules) {
		const declarations = run(cli, ['docs', name]).split('\n').slice(1).join('\n');
		out.push(`## \`${name}\``, '', description, '', ...describeDeclarations(declarations, 3));
	}
	return out.join('\n');
}

function builtins(): string {
	const catalog = run(cli, ['builtins']);
	const names = catalog
		.split('\n')
		.flatMap((line) => line.replace(/^\w+:\s*/, '').split(','))
		.map((name) => name.trim())
		.filter(Boolean);
	const out: string[] = [];
	for (const name of names) {
		const declarations = run(cli, ['builtins', name]);
		const source = ts.createSourceFile('builtins.d.ts', declarations, ts.ScriptTarget.Latest, true);
		const top = source.statements.find((statement) =>
			(ts.isInterfaceDeclaration(statement) || ts.isModuleDeclaration(statement) || ts.isClassDeclaration(statement)) &&
			statement.name?.getText(source) === name);
		out.push(`## \`${name}\``, '');
		const doc = top ? docOf(top).summary : '';
		if (doc) out.push(doc, '');
		const rest = source.statements.filter((statement) => statement !== top);
		if (top && (ts.isInterfaceDeclaration(top) || ts.isClassDeclaration(top))) {
			out.push(...memberTable(top.members, source));
		} else if (top && ts.isModuleDeclaration(top) && top.body && ts.isModuleBlock(top.body)) {
			out.push(...describeStatements(top.body.statements, source, 3, `${name}.`));
		}
		out.push(...describeStatements(rest, source, 3, ''));
	}
	return out.join('\n');
}

// ── Markdown helpers ─────────────────────────────────────────────────────

/** Inline code that survives a GFM table cell. */
function code(text: string): string {
	const ticks = text.includes('`') ? '``' : '`';
	const pad = text.startsWith('`') || text.endsWith('`') ? ' ' : '';
	return `${ticks}${pad}${text.replace(/\|/g, '\\|')}${pad}${ticks}`;
}

function cell(text: string): string {
	return text.replace(/\|/g, '\\|').replace(/\n/g, ' ');
}

// ── Regions ──────────────────────────────────────────────────────────────

const generators: Record<string, () => string> = {
	cli: cliReference,
	'server-cli': serverReference,
	capabilities,
	stdlib: standardLibrary,
	builtins,
};

const cache = new Map<string, string>();
function generated(name: string): string {
	const generator = generators[name];
	if (!generator) throw new Error(`no generator named \`${name}\``);
	if (!cache.has(name)) cache.set(name, generator().trim());
	return cache.get(name)!;
}

try {
	const files = readdirSync(docs, { recursive: true, encoding: 'utf8' })
		.filter((file) => file.endsWith('.md') && !file.startsWith('old/'));
	let changed = 0;
	for (const file of files) {
		const path = new URL(file, docs);
		const before = readFileSync(path, 'utf8');
		const after = before.replace(
			/(<!-- generated:([a-z-]+) -->\n)[\s\S]*?(<!-- \/generated:\2 -->)/g,
			(_, open: string, name: string, close: string) => `${open}\n${generated(name)}\n\n${close}`,
		);
		if (after !== before) {
			writeFileSync(path, after);
			changed++;
			console.log(`updated ${file}`);
		}
	}
	console.log(`${changed} page(s) updated`);
} finally {
	rmSync(home, { recursive: true, force: true });
}
