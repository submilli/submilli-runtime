import assert from 'node:assert/strict';
import test from 'node:test';
import { documentationSchema, serializeStructuredData } from '../src/lib/structured-data.ts';

test('nested documentation uses canonical URLs and linked breadcrumbs', () => {
	const result = documentationSchema({ slug: 'blueprints/allow-git', title: 'Allow Git', description: 'Grant Git access.' });
	assert.equal(result['@context'], 'https://schema.org');
	const [page, breadcrumbs] = JSON.parse(JSON.stringify(result))['@graph'];
	assert.equal(page['@type'], 'TechArticle');
	assert.equal(page.url, 'https://submilli.ai/docs/blueprints/allow-git/');
	assert.equal(page.mainEntityOfPage.breadcrumb['@id'], breadcrumbs['@id']);
	assert.deepEqual(breadcrumbs.itemListElement.map((item: { item: string }) => item.item), ['https://submilli.ai/docs/', page.url]);
	assert.equal(page.publisher['@id'], 'https://submilli.ai/#organization');
});

test('docs index describes a collection without duplicate breadcrumbs', () => {
	const result = documentationSchema({ slug: '', title: 'Documentation', description: 'Submilli docs' });
	assert.equal(result['@graph'].length, 1);
	assert.equal(result['@graph'][0]['@type'], 'CollectionPage');
	assert.equal(result['@graph'][0]['@id'], 'https://submilli.ai/docs/#page');
});

test('metadata cannot terminate the JSON-LD script', () => {
	const value = { title: '</script><script>alert("x")</script> & 中文' };
	const serialized = serializeStructuredData(value);
	assert.ok(!serialized.includes('<'));
	assert.deepEqual(JSON.parse(serialized), value);
});
