/**
 * Build-time GitHub star count for the header link.
 *
 * The count is fetched once per build and shared by every page. Any failure
 * (offline CI, rate limit, timeout) resolves to `null` so the header simply
 * omits the count segment instead of failing the build.
 */
export const GITHUB_REPO = 'submilli/submilli-runtime';
export const GITHUB_URL = `https://github.com/${GITHUB_REPO}`;

let cached: Promise<number | null> | undefined;

export function getGitHubStars(): Promise<number | null> {
	if (!cached) cached = fetchStars();
	return cached;
}

async function fetchStars(): Promise<number | null> {
	if (process.env.SUBMILLI_DOCS_OFFLINE) return null;
	try {
		const response = await fetch(`https://api.github.com/repos/${GITHUB_REPO}`, {
			headers: { accept: 'application/vnd.github+json' },
			signal: AbortSignal.timeout(4000),
		});
		if (!response.ok) return null;
		const data: unknown = await response.json();
		const count =
			data && typeof data === 'object' && 'stargazers_count' in data
				? (data as { stargazers_count: unknown }).stargazers_count
				: null;
		return typeof count === 'number' ? count : null;
	} catch {
		return null;
	}
}

/** 1234 → "1.2k", 12345 → "12k", 999 → "999". */
export function formatStars(count: number): string {
	if (count < 1000) return String(count);
	const thousands = count / 1000;
	const digits = thousands >= 10 ? 0 : 1;
	return `${thousands.toFixed(digits).replace(/\.0$/, '')}k`;
}
