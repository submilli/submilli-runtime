import { spawnSync } from 'node:child_process';
import { basename, dirname, resolve } from 'node:path';

/**
 * Newest commit date of a content file, read from git at build time.
 *
 * Starlight's own `lastUpdated` support only scans the history of its default
 * docs directory, and this book lives outside the site in `../docs`, so the
 * footer asks git directly. Results are cached per build; a file with no
 * history (new, uncommitted) yields `undefined` and the line is omitted.
 */
const cache = new Map<string, Date | undefined>();

export function getNewestCommitDate(filePath: string | undefined): Date | undefined {
	if (!filePath) return undefined;
	const absolute = resolve(filePath);
	if (cache.has(absolute)) return cache.get(absolute);
	const date = readCommitDate(absolute);
	cache.set(absolute, date);
	return date;
}

function readCommitDate(absolute: string): Date | undefined {
	try {
		const result = spawnSync('git', ['log', '--format=%ct', '--max-count=1', '--', basename(absolute)], {
			cwd: dirname(absolute),
			encoding: 'utf-8',
		});
		if (result.error || result.status !== 0) return undefined;
		const seconds = Number(result.stdout.trim());
		return Number.isFinite(seconds) && seconds > 0 ? new Date(seconds * 1000) : undefined;
	} catch {
		return undefined;
	}
}
