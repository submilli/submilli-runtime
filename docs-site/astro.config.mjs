// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import { satteri } from "@astrojs/markdown-satteri";
import { submilliDark, submilliLight } from "./src/code-themes.mjs";
import { securityAside } from "./src/plugins/satteri-security-aside.mjs";
import { agentDocs } from "./src/plugins/agent-docs.ts";

export default defineConfig({
  site: "https://submilli.ai",
  base: "/docs",
  // Match the URL prefix on static hosts that publish dist/ at the domain root.
  outDir: "./dist/docs",
  markdown: {
    // Starlight adds its own aside plugin after ours; `:::security` is the
    // fifth callout the design specifies for runtime-enforcement facts.
    processor: satteri({ mdastPlugins: [securityAside()] }),
  },
  integrations: [
    agentDocs(),
    starlight({
      title: "Submilli",
      description:
        "The runtime for the code your agent writes — under rules you set.",
      customCss: [
        // Self-hosted fallbacks; SF Pro Rounded wins on Apple devices via the font stack.
        "@fontsource-variable/nunito",
        "@fontsource/ibm-plex-mono/400.css",
        "@fontsource/ibm-plex-mono/500.css",
        "@fontsource/ibm-plex-mono/600.css",
        "./src/styles/theme.css",
      ],
      lastUpdated: true,
      // The book lives outside the default docs collection directory, so
      // Starlight must be told to run its Markdown transforms (asides,
      // heading anchors) on it.
      markdown: { processedDirs: ["../docs"] },
      components: {
        Head: "./src/components/Head.astro",
        Header: "./src/components/Header.astro",
        SiteTitle: "./src/components/SiteTitle.astro",
        SocialIcons: "./src/components/GitHubLink.astro",
        ThemeSelect: "./src/components/ThemeSelect.astro",
        MobileMenuFooter: "./src/components/MobileMenuFooter.astro",
        PageTitle: "./src/components/PageTitle.astro",
        Hero: "./src/components/Hero.astro",
        Footer: "./src/components/Footer.astro",
        Pagination: "./src/components/Pagination.astro",
        EditLink: "./src/components/EditLink.astro",
        LastUpdated: "./src/components/LastUpdated.astro",
      },
      expressiveCode: {
        themes: [submilliDark, submilliLight],
        useStarlightDarkModeSwitch: true,
        useStarlightUiThemeColors: false,
        styleOverrides: {
          borderRadius: "12px",
          borderWidth: "1px",
          borderColor: "var(--sl-color-hairline)",
          codeFontFamily: "var(--sl-font-mono)",
          codeFontSize: "0.875rem",
          codeLineHeight: "1.75",
          codePaddingBlock: "0.875rem",
          codePaddingInline: "1.125rem",
          uiFontFamily: "var(--sl-font-mono)",
          uiFontSize: "0.78125rem",
          focusBorder: "var(--sl-color-accent)",
          frames: {
            editorBackground: "var(--sub-code-bg)",
            terminalBackground: "var(--sub-code-bg)",
            editorTabBarBackground: "transparent",
            editorActiveTabBackground: "transparent",
            editorActiveTabForeground: "var(--sl-color-gray-2)",
            editorActiveTabIndicatorTopColor: "transparent",
            editorActiveTabIndicatorBottomColor: "transparent",
            editorActiveTabIndicatorHeight: "0",
            editorTabBarBorderBottomColor: "var(--sl-color-hairline)",
            editorTabBorderRadius: "0",
            editorTabsMarginInlineStart: "0.5rem",
            terminalTitlebarBackground: "transparent",
            terminalTitlebarForeground: "var(--sl-color-gray-2)",
            terminalTitlebarBorderBottomColor: "var(--sl-color-hairline)",
            terminalTitlebarDotsOpacity: "0",
            frameBoxShadowCssValue: "none",
            inlineButtonBackground: "transparent",
            inlineButtonBackgroundIdleOpacity: "0",
            inlineButtonBackgroundHoverOrFocusOpacity: "0",
            inlineButtonBorder: "var(--sl-color-hairline)",
            inlineButtonBorderOpacity: "1",
            inlineButtonForeground: "var(--sl-color-gray-2)",
            tooltipSuccessBackground: "var(--sub-tip)",
            tooltipSuccessForeground: "#000000",
          },
          textMarkers: {
            markBackground: "var(--sub-code-hl)",
            markBorderColor: "var(--sl-color-text-accent)",
            lineMarkerAccentWidth: "2px",
          },
        },
      },
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
          href: "https://github.com/submilli/submilli-runtime",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/submilli/submilli-runtime/edit/main/docs-site/",
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
