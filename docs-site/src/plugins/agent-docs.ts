import { mkdir, writeFile } from 'node:fs/promises';
import type { AstroIntegration } from 'astro';
import { createAgentDocs, readChapters } from '../lib/agent-docs.ts';

export function agentDocs(): AstroIntegration {
	return {
		name: 'submilli-agent-docs',
		hooks: {
			'astro:build:done': async ({ dir, logger }) => {
				// The host publishes dist/, while Astro's outDir is dist/docs/.
				const publishDirectory = new URL('../', dir);
				const outputs = createAgentDocs(await readChapters());
				for (const [path, content] of outputs) {
					const destination = new URL(`.${path}`, publishDirectory);
					await mkdir(new URL('./', destination), { recursive: true });
					await writeFile(destination, content);
				}
				logger.info(`Published ${outputs.size - 2} Markdown chapters and two agent indexes`);
			},
		},
	};
}
