const campaignKeys = ['utm_source', 'utm_medium', 'utm_campaign', 'utm_content', 'utm_term'];

export function safeUrl(value: string, baseUrl = 'https://submilli.ai/docs/') {
	try {
		const url = new URL(value, baseUrl);
		return `${url.origin}${url.pathname}`;
	} catch {
		return '';
	}
}

export function attributionFor(url: string, referrer = '') {
	const page = new URL(url);
	const attribution: Record<string, string> = { landing_path: page.pathname };
	for (const campaignKey of campaignKeys) {
		const value = page.searchParams.get(campaignKey);
		if (value && !value.includes('@')) attribution[campaignKey] = value.slice(0, 150);
	}
	try {
		const source = new URL(referrer);
		if (source.hostname !== page.hostname) attribution.referrer_host = source.hostname;
	} catch {
		// An absent or malformed referrer has no attribution value.
	}
	return attribution;
}

export function mergeAttribution(previous: { first?: unknown; latest?: unknown } | null, current: Record<string, string>) {
	const hasSource = Boolean(current.utm_source || current.referrer_host);
	return {
		first: previous?.first ?? current,
		latest: hasSource ? current : (previous?.latest ?? current),
	};
}
