import posthog from 'posthog-js';
import { attributionFor, mergeAttribution, safeUrl } from './analytics-utils';

const key = import.meta.env.PUBLIC_POSTHOG_KEY;
const host = import.meta.env.PUBLIC_POSTHOG_HOST;
const consentKey = 'submilli.analytics-consent.v1';
const attributionKey = 'submilli.attribution.v1';
let active = false;
let initialized = false;
let lastPageviewUrl: string | undefined;
let isTest = new URLSearchParams(location.search).get('analytics_test') === '1';
isTest ||= hasVerificationAttribution();
try {
	if (isTest) sessionStorage.setItem('submilli.analytics-test', '1');
	isTest ||= sessionStorage.getItem('submilli.analytics-test') === '1';
} catch {}

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

function hasVerificationAttribution() {
	const markers = new Set(['e2e', 'deployment_verification', 'verification', 'funnel_test', 'utm_e2e']);
	const marked = (touch: unknown) => touch && typeof touch === 'object'
		&& Object.entries(touch).some(([key, value]) => key.startsWith('utm_') && markers.has(String(value).trim().toLowerCase()));
	try {
		if (marked(Object.fromEntries(new URL(location.href).searchParams))) return true;
		const prior = JSON.parse(readStorage(attributionKey) || 'null');
		return Boolean(marked(prior?.first) || marked(prior?.latest));
	} catch {
		return false;
	}
}

function applyQueryOptOut() {
	try {
		if (new URL(location.href).searchParams.get('analytics') !== 'off') return false;
		writeStorage(consentKey, 'denied');
		return true;
	} catch {}
	return false;
}
function isProductionAnalyticsHost() {
	return location.hostname === 'submilli.ai';
}

function capture(event: string, properties: Record<string, unknown> = {}) {
	if (!active || posthog.has_opted_out_capturing()) return;
	try {
		posthog.capture(event, {
			schema_version: 1,
			is_test: isTest,
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
			if (cta === 'meeting') {
				capture('booking_cta_clicked', {
					cta_location: anchor.dataset.analyticsLocation || 'docs',
					booking_provider: 'google_calendar',
					destination,
					external: url.host !== location.host,
				});
			}
		}
	});
}

function start(explicitConsent = false) {
	if (active || !key || !host || isTest || !isProductionAnalyticsHost()) return;
	if (!initialized) {
		posthog.init(key, {
			api_host: host,
			autocapture: false,
			save_referrer: false,
			save_campaign_params: false,
			capture_pageview: false,
			capture_pageleave: false,
			disable_session_recording: location.hostname !== 'submilli.ai' || isTest,
			enable_recording_console_log: false,
			capture_exceptions: false,
			session_recording: {
				maskAllInputs: false,
				maskInputOptions: { password: false },
				maskTextClass: '',
				maskTextSelector: '',
				blockClass: '',
				blockSelector: '',
				recordHeaders: false,
				recordBody: false,
			},
			disable_surveys: true,
			advanced_disable_feature_flags: true,
			person_profiles: 'identified_only',
			persistence: 'localStorage',
			before_send(event) {
				if (!event) return event;
				event.properties.is_test = isTest;
				for (const field of ['$current_url', '$referrer']) {
					if (event.properties[field]) event.properties[field] = safeUrl(event.properties[field]);
				}
				return event;
			},
		});
		initialized = true;
	}
	if (explicitConsent) posthog.opt_in_capturing({ captureEventName: false });
	if (posthog.has_opted_out_capturing()) return;
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

const queryOptOut = applyQueryOptOut();

if (key && host) {
	const choice = document.getElementById('analytics-choice');
	const settings = document.getElementById('analytics-settings');
	if (settings) settings.hidden = false;
	if (!queryOptOut && readStorage(consentKey) !== 'denied') start();
	settings?.addEventListener('click', () => { if (choice) choice.hidden = false; });
	document.querySelectorAll<HTMLButtonElement>('[data-consent]').forEach(button => {
		button.addEventListener('click', () => {
			const consent = button.dataset.consent;
			if (!consent) return;
			writeStorage(consentKey, consent);
			if (consent === 'granted') {
				start(true);
			}
			else {
				active = false;
				if (initialized) {
					posthog.stopSessionRecording();
					posthog.opt_out_capturing();
				}
				try { localStorage.removeItem(attributionKey); } catch {}
			}
			if (choice) choice.hidden = true;
		});
	});
	instrumentClicks();
	document.addEventListener('astro:page-load', trackPageview);
	window.addEventListener('storage', event => {
		if ((event.key === consentKey && event.newValue === 'denied') || (event.key === null && readStorage(consentKey) === 'denied')) {
			active = false;
			if (initialized) {
				posthog.stopSessionRecording();
				posthog.opt_out_capturing();
			}
		}
	});
}
