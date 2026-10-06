export type CopyMethod = 'clipboard' | 'execCommand' | 'manual';

type MarkdownResponse = {
	ok: boolean;
	status: number;
	headers: { get(name: string): string | null };
	text(): Promise<string>;
};

type MarkdownFetch = (input: string, init: RequestInit) => Promise<MarkdownResponse>;

export async function fetchMarkdown(fetcher: MarkdownFetch, url: string, signal: AbortSignal): Promise<string> {
	const response = await fetcher(url, {
		signal,
		credentials: 'same-origin',
		headers: { Accept: 'text/markdown' },
	});
	if (!response.ok) throw new Error(`Markdown request failed (${response.status})`);
	const contentType = response.headers.get('content-type')?.toLowerCase() ?? '';
	if (!contentType.includes('text/markdown') && !contentType.includes('text/plain')) throw new Error('The response was not Markdown');
	const markdown = await response.text();
	if (!markdown.trim()) throw new Error('The Markdown response was empty');
	return markdown;
}

export interface CopyEnvironment {
	navigator: { clipboard?: { writeText(value: string): Promise<void> } };
	document: {
		body: { append(node: unknown): void } | null;
		activeElement?: unknown;
		createElement(tagName: 'textarea'): {
			value: string;
			readOnly: boolean;
			setAttribute(name: string, value: string): void;
			focus(): void;
			select(): void;
			remove(): void;
			style: { position: string; opacity: string };
		};
		execCommand?: (command: string) => boolean;
	};
}

export async function copyText(text: string, environment: CopyEnvironment): Promise<CopyMethod> {
	try {
		if (environment.navigator.clipboard) {
			await environment.navigator.clipboard.writeText(text);
			return 'clipboard';
		}
	} catch {
		// Fall through to the legacy copy path, then the explicit manual fallback.
	}

	const body = environment.document.body;
	if (!body) return 'manual';
	const textarea = environment.document.createElement('textarea');
	const previousFocus = environment.document.activeElement;
	textarea.value = text;
	textarea.readOnly = true;
	textarea.setAttribute('aria-hidden', 'true');
	textarea.style.position = 'fixed';
	textarea.style.opacity = '0';
	body.append(textarea);
	textarea.focus();
	textarea.select();
	let copied = false;
	try {
		copied = environment.document.execCommand?.('copy') === true;
	} catch {
		copied = false;
	} finally {
		textarea.remove();
		if (previousFocus && typeof previousFocus === 'object' && 'focus' in previousFocus && typeof previousFocus.focus === 'function') {
			previousFocus.focus();
		}
	}
	return copied ? 'execCommand' : 'manual';
}
