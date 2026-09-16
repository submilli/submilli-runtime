// Expressive Code / Shiki themes for the documentation.
//
// Both themes use the same small vocabulary the design specifies: keywords in
// the brand blue, strings in the refraction cyan, comments in a quiet grey and
// punctuation slightly dimmed. Everything else stays in the body text colour so
// code reads as text with a few accents rather than a rainbow.

function makeTheme({ name, type, background, foreground, keyword, string, comment, punctuation, muted }) {
	const rule = (scope, foreground, fontStyle) => ({
		scope,
		settings: fontStyle ? { foreground, fontStyle } : { foreground },
	});
	return {
		name,
		type,
		colors: {
			'editor.background': background,
			'editor.foreground': foreground,
			'editorLineNumber.foreground': muted,
			'editorLineNumber.activeForeground': foreground,
		},
		tokenColors: [
			rule(['comment', 'punctuation.definition.comment', 'string.comment'], comment),
			rule(
				[
					'keyword',
					'storage',
					'storage.type',
					'storage.modifier',
					'keyword.control',
					'keyword.operator.new',
					'keyword.operator.expression',
					'keyword.operator.logical.python',
					'constant.language',
					'support.type.primitive',
					'support.type.builtin',
					'variable.language',
					'entity.name.tag',
					'entity.other.attribute-name',
					'support.function.builtin.shell',
				],
				keyword,
			),
			rule(
				[
					'string',
					'string.template',
					'punctuation.definition.string',
					'constant.numeric',
					'constant.character',
					'constant.other',
					'string.unquoted',
					'meta.embedded.line',
				],
				string,
			),
			rule(
				[
					'punctuation',
					'keyword.operator',
					'punctuation.separator',
					'punctuation.terminator',
					'meta.brace',
					'punctuation.definition.tag',
				],
				punctuation,
			),
			rule(
				[
					'entity.name.function',
					'support.function',
					'variable',
					'variable.other',
					'entity.name.type',
					'entity.name.class',
					'support.class',
					'support.type',
					'meta.function-call',
				],
				foreground,
			),
			rule(['markup.heading', 'markup.bold'], foreground, 'bold'),
			rule(['markup.italic'], foreground, 'italic'),
			rule(['invalid', 'invalid.illegal'], type === 'dark' ? '#ff8181' : '#b42318'),
		],
	};
}

export const submilliDark = makeTheme({
	name: 'submilli-dark',
	type: 'dark',
	background: '#1b1c1f',
	foreground: '#ffffff',
	keyword: '#c4e4f9',
	string: '#88e5ff',
	comment: '#8f8f8f',
	punctuation: '#a6a7aa',
	muted: '#9a9a9a',
});

export const submilliLight = makeTheme({
	name: 'submilli-light',
	type: 'light',
	background: '#f3f8fc',
	foreground: '#202020',
	keyword: '#2b6d99',
	string: '#0f6f8f',
	comment: '#6b6b6b',
	punctuation: '#5a5a5a',
	muted: '#6b6b6b',
});
