const DISMISSAL_KEY = 'submilli.docs-invitation.dismissed.v1';
const ELAPSED_KEY = 'submilli.docs-invitation.elapsed.v1';
const ENGAGEMENT_KEY = 'submilli.docs-invitation.engaged.v1';
const VISIBLE_DELAY_MS = 10_000;
const REQUEST_TIMEOUT_MS = 15_000;

type InvitationTarget = HTMLElement & { dataset: DOMStringMap & { endpoint?: string; initialized?: string } };
const emailPattern = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

function readElapsed(storage: Storage | undefined) {
	try { const value = Number(storage?.getItem(ELAPSED_KEY)); return Number.isFinite(value) && value >= 0 ? value : 0; } catch { return 0; }
}
function readStoredFlag(storage: Storage | undefined, key: string) { try { return storage?.getItem(key) === '1'; } catch { return false; } }
function storeFlag(storage: Storage | undefined, key: string) { try { storage?.setItem(key, '1'); } catch {} }
function setElapsed(storage: Storage | undefined, elapsed: number) { try { storage?.setItem(ELAPSED_KEY, String(Math.round(elapsed))); } catch {} }
function parseSuccess(response: Response) {
	if (!response.ok) return Promise.resolve(false);
	return response.json().then((body: unknown) => Boolean(body && typeof body === 'object' && (body as { ok?: unknown }).ok === true)).catch(() => false);
}

export function createDocsInvitationController({
	document, fetch, localStorage, sessionStorage, setTimeout, clearTimeout,
	visible = () => document.visibilityState !== 'hidden', now = () => Date.now(),
	abortController = globalThis.AbortController, mutationObserver = globalThis.MutationObserver,
}: {
	document: Document; fetch: typeof globalThis.fetch; localStorage?: Storage; sessionStorage?: Storage;
	setTimeout?: typeof globalThis.setTimeout; clearTimeout?: typeof globalThis.clearTimeout;
	visible?: () => boolean; now?: () => number; abortController?: typeof globalThis.AbortController;
	mutationObserver?: typeof globalThis.MutationObserver;
}) {
	let elapsed = readElapsed(sessionStorage);
	let engaged = readStoredFlag(sessionStorage, ENGAGEMENT_KEY);
	let scrollIntent: { from: number; until: number } | undefined;
	let lastStartedAt: number | undefined;
	let timer: ReturnType<typeof globalThis.setTimeout> | undefined;
	let dismissed = readStoredFlag(localStorage, DISMISSAL_KEY);
	let current: InvitationTarget | undefined;
	let overlayObserver: MutationObserver | undefined;
	let activeRequest: { controller?: AbortController; timeout: ReturnType<typeof globalThis.setTimeout> | undefined } | undefined;

	const disconnectOverlayObserver = () => {
		overlayObserver?.disconnect();
		overlayObserver = undefined;
	};
	const abortRequest = () => {
		if (!activeRequest) return;
		activeRequest?.controller?.abort();
		if (activeRequest?.timeout) (clearTimeout || globalThis.clearTimeout)(activeRequest.timeout);
		activeRequest = undefined;
		const submit = current?.querySelector<HTMLButtonElement>('[data-invitation-submit]');
		if (submit) { submit.disabled = false; submit.textContent = 'Let’s chat'; }
	};

	const stopTimer = () => {
		if (lastStartedAt !== undefined) { elapsed += now() - lastStartedAt; setElapsed(sessionStorage, elapsed); }
		lastStartedAt = undefined;
		if (timer) (clearTimeout || globalThis.clearTimeout)(timer);
		timer = undefined;
	};
	const overlaysVisible = () => {
		const analyticsChoice = document.getElementById('analytics-choice');
		if (analyticsChoice && !analyticsChoice.hidden) return true;
		return Boolean(document.querySelector('dialog[open], [aria-modal="true"]:not([hidden])'));
	};
	const showWhenClear = () => {
		if (!current || dismissed || !engaged || elapsed < VISIBLE_DELAY_MS || !visible() || overlaysVisible()) return;
		current.hidden = false;
		disconnectOverlayObserver();
	};
	const watchOverlays = () => {
		if (overlayObserver || !mutationObserver || !document.body) return;
		overlayObserver = new mutationObserver(() => showWhenClear());
		overlayObserver.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['hidden', 'open', 'aria-hidden'] });
	};
	const revealOrWatch = () => {
		if (!engaged || elapsed < VISIBLE_DELAY_MS || !visible()) return;
		if (overlaysVisible()) watchOverlays(); else showWhenClear();
	};
	const schedule = () => {
		if (timer || dismissed || elapsed >= VISIBLE_DELAY_MS || !current || !visible()) return;
		lastStartedAt = now();
		timer = (setTimeout || globalThis.setTimeout)(() => {
			timer = undefined;
			if (lastStartedAt !== undefined) elapsed += now() - lastStartedAt;
			lastStartedAt = undefined; setElapsed(sessionStorage, elapsed);
			if (elapsed >= VISIBLE_DELAY_MS) revealOrWatch(); else schedule();
		}, VISIBLE_DELAY_MS - elapsed);
	};
	const recordEngagement = () => {
		if (!current || dismissed || !visible() || engaged) return;
		engaged = true; storeFlag(sessionStorage, ENGAGEMENT_KEY);
		if (elapsed >= VISIBLE_DELAY_MS) revealOrWatch();
	};
	const onClick = (event: MouseEvent) => {
		if (!event.isTrusted || event.button !== 0 || !current) return;
		const target = event.target as Element | null;
		if (target?.closest?.('[data-docs-invitation]')) return;
		if (target?.closest?.('[data-copy-page], [data-copy-setup], .section-link-copy, .expressive-code .copy button')) {
			recordEngagement(); return;
		}
		const link = target?.closest?.('a[href]');
		if (!link) return;
		try {
			const destination = new URL(link.getAttribute('href') || '', document.baseURI);
			const page = new URL(document.baseURI);
			if (destination.origin === page.origin && destination.pathname.startsWith('/docs/')
				&& (destination.pathname !== page.pathname || destination.hash && destination.hash !== page.hash)) recordEngagement();
		} catch { /* A malformed link is not engagement. */ }
	};
	const onSearch = (event: Event) => {
		const input = event.target as HTMLInputElement | null;
		if (event.isTrusted && input?.matches?.('input[type="search"]') && input.value.trim().length >= 2) recordEngagement();
	};
	const onScrollIntent = (event: Event) => {
		if (!event.isTrusted || !current || dismissed || engaged || !visible()) return;
		if (event.type === 'keydown') {
			const key = event as KeyboardEvent;
			const target = event.target as Element | null;
			if (key.altKey || key.ctrlKey || key.metaKey || !['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End', ' '].includes(key.key)
				|| target?.closest?.('input, textarea, select, [contenteditable="true"]')) return;
		}
		const from = document.scrollingElement?.scrollTop ?? 0;
		if (!scrollIntent || now() > scrollIntent.until) scrollIntent = { from, until: now() + 1500 };
		else scrollIntent.until = now() + 1500;
	};
	const onScroll = () => {
		// Restoration and scripted scrolling alone are not reader engagement.
		if (scrollIntent && now() <= scrollIntent.until && Math.abs((document.scrollingElement?.scrollTop ?? 0) - scrollIntent.from) >= 80) recordEngagement();
	};
	document.addEventListener('click', onClick, true);
	document.addEventListener('input', onSearch);
	document.addEventListener('wheel', onScrollIntent, { passive: true });
	document.addEventListener('touchmove', onScrollIntent, { passive: true });
	document.addEventListener('keydown', onScrollIntent);
	document.addEventListener('scroll', onScroll, true);

	const bind = () => {
		const next = document.querySelector<InvitationTarget>('[data-docs-invitation]');
		if (current && current !== next) abortRequest();
		if (!next) { stopTimer(); abortRequest(); current = undefined; disconnectOverlayObserver(); return; }
		current = next;
		if (next.dataset.initialized === 'true') { if (elapsed >= VISIBLE_DELAY_MS) revealOrWatch(); else schedule(); return; }
		next.dataset.initialized = 'true';
		if (dismissed) { next.remove(); current = undefined; return; }
		const form = next.querySelector<HTMLFormElement>('form');
		const input = next.querySelector<HTMLInputElement>('input[type="email"]');
		const submit = next.querySelector<HTMLButtonElement>('[data-invitation-submit]');
		const message = next.querySelector<HTMLElement>('[data-invitation-message]');
		input?.addEventListener('input', () => input.setCustomValidity(''));
		next.querySelector<HTMLButtonElement>('[data-invitation-dismiss]')?.addEventListener('click', () => {
			dismissed = true; stopTimer(); abortRequest(); storeFlag(localStorage, DISMISSAL_KEY); next.remove(); current = undefined;
		});
		form?.addEventListener('submit', async (event) => {
			event.preventDefault();
			if (!input || !submit || !message || submit.disabled) return;
			const email = input.value.trim();
			if (!emailPattern.test(email)) { input.setCustomValidity('Enter a valid email address.'); input.reportValidity(); return; }
			input.setCustomValidity(''); submit.disabled = true; submit.textContent = 'Sending…'; message.textContent = '';
			const request = abortController ? new abortController() : undefined;
			const requestState = { controller: request, timeout: undefined as ReturnType<typeof globalThis.setTimeout> | undefined };
			activeRequest = requestState;
			requestState.timeout = (setTimeout || globalThis.setTimeout)(() => request?.abort(), REQUEST_TIMEOUT_MS);
			try {
				const response = await fetch(next.dataset.endpoint || '', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ email, form_id: 'docs-invitation' }), credentials: 'omit', signal: request?.signal });
				if (!await parseSuccess(response)) throw new Error('submission failed');
				if (dismissed || current !== next) return;
				message.textContent = 'Thanks — we’ll be in touch.'; input.value = ''; form.hidden = true; storeFlag(localStorage, DISMISSAL_KEY); dismissed = true;
			} catch {
				if (dismissed || current !== next) return;
				message.textContent = 'We couldn’t save that. Please try again.'; submit.disabled = false; submit.textContent = 'Let’s chat';
			} finally {
				if (activeRequest === requestState) {
					if (requestState.timeout) (clearTimeout || globalThis.clearTimeout)(requestState.timeout);
					activeRequest = undefined;
				}
			}
		});
		if (elapsed >= VISIBLE_DELAY_MS) revealOrWatch(); else schedule();
	};
	return {
		bind,
		pauseForNavigation() { stopTimer(); abortRequest(); current = undefined; scrollIntent = undefined; disconnectOverlayObserver(); },
		onVisibilityChange() { if (visible()) { if (elapsed >= VISIBLE_DELAY_MS) revealOrWatch(); else schedule(); } else stopTimer(); },
		get state() { return { elapsed, engaged, dismissed, visible: Boolean(current && !current.hidden) }; },
	};
}

export function startDocsInvitation() {
	if (typeof document === 'undefined') return;
	let browserStorage: Storage | undefined; let sessionStorage: Storage | undefined;
	try { browserStorage = localStorage; } catch {} try { sessionStorage = globalThis.sessionStorage; } catch {}
	const controller = createDocsInvitationController({ document, fetch: globalThis.fetch.bind(globalThis), localStorage: browserStorage, sessionStorage });
	controller.bind();
	document.addEventListener('visibilitychange', () => controller.onVisibilityChange());
	window.addEventListener('pagehide', () => controller.pauseForNavigation());
	window.addEventListener('pageshow', () => controller.bind());
	document.addEventListener('astro:before-swap', () => controller.pauseForNavigation());
	document.addEventListener('astro:page-load', () => controller.bind());
}
