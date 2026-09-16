// Sätteri mdast plugin adding a fifth aside, `:::security`, for facts the
// runtime enforces. It emits the same markup as Starlight's built-in asides
// (see `starlight-asides` in @astrojs/starlight) so the shared aside styles
// apply, with a `--security` modifier class and a shield glyph.
//
// Usage in Markdown:
//
//   :::security
//   Deny is the default. Nothing runs unless a policy names it.
//   :::
//
//   :::security[Custom title]
//   ...
//   :::

const SHIELD_ICON =
	'<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="starlight-aside__icon">' +
	'<path d="M12 2 4 5v6c0 5 3.4 9.4 8 11 4.6-1.6 8-6 8-11V5l-8-3Z"></path><path d="m9 12 2 2 4-4"></path></svg>';

/** Builds an mdast paragraph that rehype renders as the given element. */
function element(tagName, properties, children) {
	return { type: 'paragraph', data: { hName: tagName, hProperties: properties }, children };
}

/** @returns {import('satteri').MdastPluginDefinition} */
export function securityAside() {
	return {
		name: 'submilli-security-aside',
		containerDirective(node, ctx) {
			if (node.name !== 'security') return;

			let title = 'Security';
			let titleNode = [{ type: 'text', value: title }];
			const children = [...node.children];
			const first = children[0];
			if (first?.type === 'paragraph' && first.data?.directiveLabel && first.children.length > 0) {
				titleNode = first.children;
				title = ctx.textContent(first);
				children.shift();
			}

			return element(
				'aside',
				{ 'aria-label': title, class: 'starlight-aside starlight-aside--security' },
				[
					element('p', { class: 'starlight-aside__title', 'aria-hidden': 'true' }, [
						{ type: 'html', value: SHIELD_ICON },
						...titleNode,
					]),
					element('div', { class: 'starlight-aside__content' }, children),
				],
			);
		},
	};
}
