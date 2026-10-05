import { mkdir, writeFile } from 'node:fs/promises';
import type { AstroIntegration } from 'astro';
import { legacyDocsRoutes } from '../lib/legacy-docs.ts';
import { createAgentDocs, readChapters } from '../lib/agent-docs.ts';

export function agentDocs(): AstroIntegration {
	return {
		name: 'submilli-agent-docs',
		hooks: {
			'astro:build:done': async ({ dir, logger }) => {
				// The host publishes dist/, while Astro's outDir is dist/docs/.
				const publishDirectory = new URL('../', dir);
				const outputs = createAgentDocs(await readChapters());
				const chapterCount = outputs.size - 2;
				// Plain Markdown cannot redirect on a static host. Serve the current
				// chapter at old .md URLs until the host's HTTP redirects take over.
				for (const [source, target] of Object.entries(legacyDocsRoutes)) {
					const currentPath = `${target.replace(/\/$/, '')}.md`;
					const current = outputs.get(currentPath);
					if (!current) throw new Error(`Missing legacy Markdown target: ${currentPath}`);
					outputs.set(`/docs${source}.md`, current);
				}
				for (const [path, content] of outputs) {
					const destination = new URL(`.${path}`, publishDirectory);
					await mkdir(new URL('./', destination), { recursive: true });
					await writeFile(destination, content);
				}
				logger.info(`Published ${chapterCount} Markdown chapters, ${Object.keys(legacyDocsRoutes).length} legacy aliases, and two agent indexes`);
			},
		},
	};
}
