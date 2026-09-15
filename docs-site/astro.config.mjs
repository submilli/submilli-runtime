// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://submilli.ai",
  base: "/docs",
  // Match the URL prefix on static hosts that publish dist/ at the domain root.
  outDir: "./dist/docs",
  integrations: [
    starlight({
      title: "Submilli",
      description:
        "The runtime for the code your agent writes — under rules you set.",
      customCss: ["./src/styles/tokens.css"],
      // Pre-launch: the docs are reachable by direct link (design partners,
      // previews) but kept out of search. Remove this block at launch.
      head: [
        {
          tag: "meta",
          attrs: { name: "robots", content: "noindex, nofollow" },
        },
      ],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/submilli/submilli-public",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/submilli/submilli-public/edit/main/docs-site/",
      },
      sidebar: [
        { label: "Getting started", items: [{ autogenerate: { directory: "../docs/part-1-getting-started" } }] },
        { label: "Writing code", items: [{ autogenerate: { directory: "../docs/part-2-writing-code" } }] },
        { label: "Running", items: [{ autogenerate: { directory: "../docs/part-3-running" } }] },
        { label: "Security and governance", items: [{ autogenerate: { directory: "../docs/part-4-security-and-governance" } }] },
        { label: "Crafting a package", items: [{ autogenerate: { directory: "../docs/part-5-crafting-a-package" } }] },
        { label: "Going to production", items: [{ autogenerate: { directory: "../docs/part-6-going-to-production" } }] },
      ],
    }),
  ],
});
