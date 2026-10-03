import { defineCollection, z } from "astro:content";
import { glob } from "astro/loaders";
import { i18nLoader } from "@astrojs/starlight/loaders";
import { docsSchema, i18nSchema } from "@astrojs/starlight/schema";

export const collections = {
  docs: defineCollection({
    loader: glob({ pattern: "**/*.md", base: "../docs" }),
    schema: docsSchema({
      // The home page's "Next steps" cards, as slugs in the order shown.
      extend: z.object({ nextSteps: z.array(z.string()).optional() }),
    }),
  }),
  // UI strings the theme overrides (search placeholder, footer labels).
  i18n: defineCollection({ loader: i18nLoader(), schema: i18nSchema() }),
};
