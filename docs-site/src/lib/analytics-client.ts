import posthog from 'posthog-js/dist/module.no-external';
import { attributionFor, mergeAttribution, safeUrl } from './analytics-utils';

const key = import.meta.env.PUBLIC_POSTHOG_KEY;
const host = import.meta.env.PUBLIC_POSTHOG_HOST;
const consentKey = 'submilli.analytics-consent.v1';
const attributionKey = 'submilli.attribution.v1';
let active = false;
let initialized = false;
let lastPageviewUrl: string | undefined;

function readStorage(storageKey: string) {
	try {
		return localStorage.getItem(storageKey);
	} catch {
		return null;
	}
}

function writeStorage(storageKey: string, value: string) {
	try {
		localStorage.setItem(storageKey, value);
	} catch {
		// Analytics must never interrupt docs navigation.
	}
}

function stripReplayQuery(value: string) {
	try {
		const url = new URL(value, location.href);
		url.search = '';
		url.hash = '';
		return url.href;
	} catch {
		return undefined;
	}
}

function capture(event: string, properties: Record<string, unknown> = {}) {
	if (!active || posthog.has_opted_out_capturing()) return;
	try {
		posthog.capture(event, {
			schema_version: 1,
			is_test: new URLSearchParams(location.search).get('analytics_test') === '1',
			...properties,
		});
	} catch {
		// Analytics failures must not affect the documentation experience.
	}
}

function trackPageview() {
	if (!active || posthog.has_opted_out_capturing()) return;
	const currentUrl = safeUrl(location.href);
	if (!currentUrl || currentUrl === lastPageviewUrl) return;
	lastPageviewUrl = currentUrl;
	capture('$pageview', { page_path: location.pathname, $current_url: currentUrl });
}

function instrumentClicks() {
	document.addEventListener('click', (event) => {
		const anchor = event.target instanceof Element ? event.target.closest('a') : null;
		if (!anchor) return;
		let url: URL;
		try {
			url = new URL(anchor.href, location.href);
		} catch {
			return;
		}
		if (!['http:', 'https:'].includes(url.protocol)) return;
		const destination = safeUrl(url.href);
		capture('link_clicked', { destination, external: url.host !== location.host });
		const cta = anchor.dataset.analyticsCta;
		if (cta) {
			capture('docs_cta_clicked', {
				cta_id: cta,
				destination,
				external: url.host !== location.host,
			});
		}
	});
}

function start() {
	if (active || !key || !host) return;
	if (!initialized) {
		posthog.init(key, {
			api_host: host,
			autocapture: false,
			save_referrer: false,
			save_campaign_params: false,
			capture_pageview: false,
			capture_pageleave: false,
			disable_session_recording: true,
			enable_recording_console_log: false,
			capture_exceptions: false,
			session_recording: {
				maskAllInputs: true,
				blockSelector: 'input[type="hidden"], input[type="file"]',
				maskAttributeFn(name, value) {
					return ['action', 'href', 'src'].includes(name.toLowerCase()) ? stripReplayQuery(value) || '' : value;
				},
				recordHeaders: false,
				recordBody: false,
				maskCapturedNetworkRequestFn(request) {
					const name = stripReplayQuery(request.name);
					return name ? { ...request, name } : undefined;
				},
			},
			disable_surveys: true,
			advanced_disable_feature_flags: true,
			person_profiles: 'identified_only',
			persistence: 'localStorage',
			before_send(event) {
				if (!event) return event;
				for (const field of ['$current_url', '$referrer']) {
					if (event.properties[field]) event.properties[field] = safeUrl(event.properties[field]);
				}
				return event;
			},
		});
		initialized = true;
	}
	posthog.opt_in_capturing({ captureEventName: false });
	active = true;
	let previous: { first?: unknown; latest?: unknown } | null = null;
	try {
		previous = JSON.parse(readStorage(attributionKey) || 'null');
	} catch {
		// Ignore malformed local attribution and start a fresh record.
	}
	const attribution = mergeAttribution(previous, attributionFor(location.href, document.referrer));
	writeStorage(attributionKey, JSON.stringify(attribution));
	posthog.register({ first_touch: attribution.first, latest_touch: attribution.latest });
	trackPageview();
}

if (key && host) {
	const choice = document.getElementById('analytics-choice');
	const settings = document.getElementById('analytics-settings');
	if (settings) settings.hidden = false;
	if (readStorage(consentKey) === 'granted') start();
	else if (!readStorage(consentKey) && choice) choice.hidden = false;
	settings?.addEventListener('click', () => { if (choice) choice.hidden = false; });
	document.querySelectorAll<HTMLButtonElement>('[data-consent]').forEach(button => {
		button.addEventListener('click', () => {
			const consent = button.dataset.consent;
			if (!consent) return;
			writeStorage(consentKey, consent);
			if (consent === 'granted') start();
			else {
				active = false;
				if (initialized) posthog.opt_out_capturing();
				try { localStorage.removeItem(attributionKey); } catch {}
			}
			if (choice) choice.hidden = true;
		});
	});
	instrumentClicks();
	document.addEventListener('astro:page-load', trackPageview);
}
