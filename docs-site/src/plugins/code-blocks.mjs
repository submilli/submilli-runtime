/**
 * Shared code-block affordances for the docs site.
 *
 * Input/output labels are deliberately conservative. Authors can always opt in
 * with `input`, `output`, or `type="input|output"` in the fence metadata.
 */

const commandPattern = /^(?:[$>]\s*)?(?:submilli|npm|pnpm|yarn|bun|cargo|git|curl|wget|mkdir|cd|cat|export|echo|node|docker|python|pip|deno|npx|touch|cp|mv|rm)\b/;
const outputPattern = /^(?:created|updated|deleted|checked|installed|removed|skipped|success|error:|warning:|[✓✗]|ts=\d{4}-\d{2}-\d{2}T|INFO\b)/i;

function explicitRole(codeBlock) {
	const options = codeBlock.metaOptions;
	const role = options.getString('type') ?? options.getString('role') ?? options.getString('io');
	if (role) {
		const normalized = role.toLowerCase();
		if (normalized === 'input' || normalized === 'output') return normalized;
	}
	if (options.getBoolean('input') === true) return 'input';
	if (options.getBoolean('output') === true) return 'output';
	return undefined;
}

/** @param {string} code @param {string} language @param {import('@expressive-code/core').MetaOptions} metaOptions */
export function classifyCodeBlock(code, language, metaOptions) {
	const explicit = explicitRole({ metaOptions });
	if (explicit) return explicit;

	const normalizedLanguage = language.toLowerCase();
	if (normalizedLanguage === 'input' || normalizedLanguage === 'output') return normalizedLanguage;
	// Heuristics are for the unannotated plaintext fences used by the quickstart.
	// Typed source and configuration blocks must remain neutral, even if a line
	// happens to begin with a word such as `export`.
	if (normalizedLanguage !== 'plaintext' && normalizedLanguage !== '') return undefined;

	const lines = code.split('\n').map((line) => line.trim()).filter(Boolean);
	if (!lines.length) return undefined;
	if (lines.every((line) => commandPattern.test(line))) return 'input';
	if (lines.every((line) => outputPattern.test(line))) return 'output';
	return undefined;
}

function addClass(element, className) {
	const classes = Array.isArray(element.properties?.className)
		? element.properties.className
		: [];
	if (!classes.includes(className)) classes.push(className);
	element.properties = { ...element.properties, className: classes };
}

function findChild(element, tagName) {
	return element.children?.find((child) => child.type === 'element' && child.tagName === tagName);
}

export function codeBlocks() {
	return {
		name: 'Submilli code-block affordances',
		hooks: {
			postprocessRenderedBlock({ codeBlock, renderData }) {
				const role = classifyCodeBlock(codeBlock.code, codeBlock.language, codeBlock.metaOptions);
				if (!role) return;

				const frame = renderData.blockAst;
				addClass(frame, `sub-code-${role}`);
				const header = findChild(frame, 'figcaption');
				if (!header) return;
				addClass(header, 'sub-code-label-header');
				header.children = [
					{ type: 'element', tagName: 'span', properties: { className: ['sub-code-label'] }, children: [{ type: 'text', value: role === 'input' ? 'Input' : 'Output' }] },
					...header.children,
				];
			},
		},
	};
}
