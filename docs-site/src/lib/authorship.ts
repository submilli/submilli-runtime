import { createHash } from 'node:crypto';

export const authorshipLabels = {
	'human-written': {
		label: 'Human-written',
		description: 'The author wrote the prose. AI may have proofread it, but did not draft it.',
	},
	'ai-assisted': {
		label: 'AI-assisted',
		description: 'This page includes both human and AI contributions.',
	},
	'ai-generated': {
		label: 'AI-generated',
		description: 'AI generated the prose. This label does not attest to human review.',
	},
	'generated-from-source': {
		label: 'Generated from source',
		description: 'Mechanically generated from source material.',
	},
} as const;

export interface Authorship {
	label: keyof typeof authorshipLabels;
	confirmed: boolean;
	contentHash: string;
	confirmedAt: string;
}

export function confirmedAuthorshipFor(authorship: Authorship | undefined, body: string | undefined) {
	const normalized = body?.replace(/\r\n/g, '\n').trim();
	if (!authorship?.confirmed || !normalized) return undefined;
	if (createHash('sha256').update(normalized, 'utf8').digest('hex') !== authorship.contentHash) return undefined;
	return authorshipLabels[authorship.label];
}
