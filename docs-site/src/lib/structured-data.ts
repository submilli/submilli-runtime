import { DOCS_ORIGIN } from './agent-docs.ts';

interface DocumentationMetadata {
	slug: string;
	title: string;
	description: string;
}

/** Use canonical production URLs even in a PR preview. */
export function documentationSchema({ slug, title, description }: DocumentationMetadata) {
	const url = `${DOCS_ORIGIN}/docs/${slug ? `${slug}/` : ''}`;
	const page = {
		'@type': slug ? 'TechArticle' : 'CollectionPage',
		'@id': `${url}#page`,
		url,
		headline: title,
		name: title,
		description,
		inLanguage: 'en',
		publisher: { '@type': 'Organization', '@id': `${DOCS_ORIGIN}/#organization`, name: 'Submilli', url: `${DOCS_ORIGIN}/` },
		...(slug ? { mainEntityOfPage: { '@type': 'WebPage', '@id': url, breadcrumb: { '@id': `${url}#breadcrumbs` } } } : {}),
	};
	const breadcrumbs = {
		'@type': 'BreadcrumbList',
		'@id': `${url}#breadcrumbs`,
		itemListElement: [
			{ '@type': 'ListItem', position: 1, name: 'Documentation', item: `${DOCS_ORIGIN}/docs/` },
			{ '@type': 'ListItem', position: 2, name: title, item: url },
		],
	};
	return { '@context': 'https://schema.org', '@graph': slug ? [page, breadcrumbs] : [page] };
}

export function serializeStructuredData(value: unknown): string {
	// A literal closing script tag in page metadata must remain JSON data.
	return JSON.stringify(value).replace(/</g, '\\u003c');
}
