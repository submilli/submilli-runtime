import { defineCollection, z } from "astro:content";
import { glob } from "astro/loaders";
import { i18nLoader } from "@astrojs/starlight/loaders";
import { docsSchema, i18nSchema } from "@astrojs/starlight/schema";

const authorshipSchema = z.object({
  label: z.enum(["human-written", "ai-assisted", "ai-generated", "generated-from-source"]),
  confirmed: z.boolean(),
  contentHash: z.string().regex(/^[a-f0-9]{64}$/),
  confirmedAt: z.string().datetime({ offset: true }),
});

export const collections = {
  docs: defineCollection({
    loader: glob({ pattern: ["**/*.md", "!WRITING.md"], base: "../docs" }),
    schema: docsSchema({
      // The home page's "Next steps" cards, as slugs in the order shown.
      extend: z.object({
        nextSteps: z.array(z.string()).optional(),
        authorship: authorshipSchema.optional(),
      }),
    }),
  }),
  // UI strings the theme overrides (search placeholder, footer labels).
  i18n: defineCollection({ loader: i18nLoader(), schema: i18nSchema() }),
};
