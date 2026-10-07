import assert from 'node:assert/strict';
import test from 'node:test';
import { attributionFor, mergeAttribution, safeUrl } from '../src/lib/analytics-utils.ts';

test('sanitizes query strings and fragments before capture', () => {
	assert.equal(safeUrl('https://submilli.ai/docs/quickstart?email=private@example.com#token'), 'https://submilli.ai/docs/quickstart');
	assert.equal(safeUrl('/docs/quickstart?utm_source=launch'), 'https://submilli.ai/docs/quickstart');
});

test('keeps campaign attribution while excluding email values', () => {
	assert.deepEqual(
		attributionFor('https://submilli.ai/docs/?utm_source=launch&utm_campaign=docs&email=private@example.com', 'https://example.com/article'),
		{ landing_path: '/docs/', utm_source: 'launch', utm_campaign: 'docs', referrer_host: 'example.com' },
	);
});

test('preserves first touch and updates latest touch only for a new source', () => {
	const first = { landing_path: '/docs/' };
	const prior = { first, latest: first };
	const direct = { landing_path: '/docs/quickstart' };
	const campaign = { landing_path: '/docs/quickstart', utm_source: 'launch' };
	assert.deepEqual(mergeAttribution(prior, direct), { first, latest: first });
	assert.deepEqual(mergeAttribution(prior, campaign), { first, latest: campaign });
});
