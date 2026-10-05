import { confirmedAuthorshipFor } from './authorship.ts';
import { readdir, readFile } from 'node:fs/promises';
import { parseFrontmatter } from 'astro/markdown';
import { parse, postprocess, preprocess } from 'micromark';
import { gfm } from 'micromark-extension-gfm';
import { decodeString } from 'micromark-util-decode-string';

export const DOCS_ORIGIN = 'https://submilli.ai';
export const DOCS_INDEX = '/docs/llms.txt';
const docsDirectory = new URL('../../../docs/', import.meta.url);

export interface Chapter {
	file: string;
	slug: string;
	title: string;
	description: string;
	order: number;
	body: string;
	authorshipLabel?: string;
}

export function markdownPath(slug: string): string {
	return `/docs/${slug || 'index'}.md`;
}

export async function readChapters(directory = docsDirectory): Promise<Chapter[]> {
	const files = (await readdir(directory, { recursive: true }))
		.filter((file) => file.endsWith('.md') && file !== 'WRITING.md').sort();
	const chapters: Chapter[] = [];
	const paths = new Set<string>();
	for (const file of files) {
		const source = await readFile(new URL(file, directory), 'utf8');
		const { frontmatter: data, content } = parseFrontmatter(source);
		if (data.sidebar?.hidden === true) continue;
		if (typeof data.slug !== 'string' ||
			(data.slug !== '' && !/^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/.test(data.slug))) {
			throw new Error(`${file}: provide a URL-safe slug for the Markdown export`);
		}
		const path = markdownPath(data.slug);
		if (paths.has(path)) throw new Error(`${file}: duplicate Markdown path ${path}`);
		paths.add(path);
		chapters.push({
			file, slug: data.slug,
			authorshipLabel: confirmedAuthorshipFor(data.authorship, content)?.label,
			title: requiredText(data.title, file, 'title'),
			description: requiredText(data.description, file, 'description'),
			order: data.sidebar?.order ?? Number.POSITIVE_INFINITY,
			// The reference generator's region markers mean nothing to a reader.
			body: content.replace(/^<!-- \/?generated:[a-z-]+ -->\n/gm, '').trim(),
		});
	}
	return chapters.sort(compareChapters);
}

export function createAgentDocs(chapters: Chapter[]): Map<string, string> {
	const outputs = new Map<string, string>();
	const visiblePaths = new Set(chapters.map((chapter) => htmlPath(chapter.slug)));
	const sections: string[] = [];
	for (const chapter of chapters) {
		const url = DOCS_ORIGIN + markdownPath(chapter.slug);
		const body = rewriteLinks(chapter.body, chapter.slug, visiblePaths);
		const disclosure = chapter.authorshipLabel ? `Authorship: ${chapter.authorshipLabel}.\n\n` : '';
		const markdown = `# ${chapter.title}\n\nSource: ${url}\n\n${disclosure}${body}\n`;
		outputs.set(markdownPath(chapter.slug), markdown);
		sections.push(markdown);
	}
	const links = chapters.map((chapter) =>
		`- [${escapeLabel(chapter.title)}](${DOCS_ORIGIN}${markdownPath(chapter.slug)}): ${chapter.description}`);
	outputs.set(DOCS_INDEX, [
		'# Submilli documentation', '',
		'> Submilli runs the programs your agent writes, under rules you set.', '',
		'Fetch the relevant Markdown chapters below. For the entire book in one fetch, use',
		`[the complete documentation](${DOCS_ORIGIN}/docs/llms-full.txt).`, '',
		'## Chapters', '', ...links, '',
	].join('\n'));
	outputs.set('/docs/llms-full.txt', sections.join('\n---\n\n'));
	return outputs;
}

function requiredText(value: unknown, file: string, field: string): string {
	if (typeof value !== 'string' || !value.trim()) {
		throw new Error(`${file}: provide a ${field} for the documentation index`);
	}
	return value.trim().replace(/\s+/g, ' ');
}

function compareChapters(left: Chapter, right: Chapter): number {
	if (left.slug === '') return -1;
	if (right.slug === '') return 1;
	const groupOrder = left.file.split('/')[0].localeCompare(right.file.split('/')[0]);
	return groupOrder || left.order - right.order || left.file.localeCompare(right.file);
}

function htmlPath(slug: string): string {
	return `/docs${slug ? `/${slug}` : ''}`;
}

function escapeLabel(value: string): string {
	return value.replace(/[\\[\]]/g, '\\$&');
}

// Destination tokens let us preserve labels, table escaping, code, and callouts verbatim.
function rewriteLinks(body: string, slug: string, visiblePaths: Set<string>): string {
	const parser = parse({ extensions: [gfm()] });
	const events = postprocess(parser.document().write(preprocess()(body, 'utf8', true)));
	const replacements: { start: number; end: number; text: string }[] = [];
	let imageDepth = 0;
	for (const [event, token] of events) {
		if (token.type === 'image') imageDepth += event === 'enter' ? 1 : -1;
		const isAutolink = ['autolinkProtocol', 'literalAutolinkHttp'].includes(token.type);
		if (event !== 'enter' ||
			(!isAutolink && token.type !== 'resourceDestinationString' && token.type !== 'definitionDestinationString')) continue;
		const start = token.start.offset;
		const end = token.end.offset;
		const raw = body.slice(start, end);
		const original = isAutolink ? raw : decodeString(raw);
		const target = resolveLink(original, slug, visiblePaths, imageDepth > 0);
		if (target !== original) {
			// Keep the destination valid in both bare and angle-bracket Markdown links.
			const escaped = target.replace(/[()<>\\|]/g, (character) =>
				`%${character.charCodeAt(0).toString(16).toUpperCase()}`);
			// Autolinks treat character references literally, unlike link destinations.
			replacements.push({ start, end, text: isAutolink ? escaped : escaped.replace(/&/g, '&amp;') });
		}
	}
	for (const { start, end, text } of replacements.sort((a, b) => b.start - a.start)) {
		body = body.slice(0, start) + text + body.slice(end);
	}
	return body;
}

function resolveLink(link: string, slug: string, visiblePaths: Set<string>, isImage: boolean): string {
	const url = URL.parse(link, DOCS_ORIGIN + htmlPath(slug));
	if (!url) return link;
	if (/^[a-z][a-z0-9+.-]*:/i.test(link) && url.origin !== DOCS_ORIGIN) return link;
	const path = url.pathname.replace(/\/$/, '');
	if (!isImage && url.origin === DOCS_ORIGIN && visiblePaths.has(path)) {
		url.pathname = markdownPath(path.slice('/docs'.length).replace(/^\//, ''));
	}
	return url.href;
}
