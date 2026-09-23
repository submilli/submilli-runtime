# Curated packages: chapter brief

- **Purpose:** Help readers choose and use the maintained service packages in this repository.
- **Starting point:** The quickstart's CLI setup, TypeScript imports and `main`, and the standard-library chapter.
- **Understanding:** Readers can distinguish packages from built-in modules, find the ten maintained packages, explain installation versus blueprint access, and distinguish program permissions, package permissions, and service credentials.
- **Action:** Install Jina, configure a blueprint that permits reading one host, run a page-reading program, and discover another package's API and setup requirements.
- **Boundaries:** Include a catalog and one complete consumption workflow. Leave policy grammar to the blueprint/security chapters, server sessions to the server/harness chapters, and package authoring to Part 5. Leave exhaustive API declarations to `submilli docs`.
- **Evidence:** Check the catalog against `submilli.toml` and package readmes; check the example against `packages/jina/src/lib.ts`, its capability schema and reader tests, and the CLI blueprint/install implementations. Run the example using a temporary package store, verify the denied-host case, and check/build the documentation site.

## Verification

- Published the local Jina package into an isolated `/tmp` package store and ran the chapter's blueprint commands.
- Ran `read-page.ts` against the live Jina service without credentials. It returned Markdown with a source URL and cached content, so the chapter does not promise a fixed page title or body.
- Ran the same program with `example.org`: it failed at the package's `jina.ai/read` check with `PermissionDeniedError` for `main`.
- Blueprint lint passed with the expected warning for the ungranted search capability. Verified removing the optional secret declaration and adding the store binding; no real credential was needed for that configuration check.
- Checked the GitHub install syntax against CLI help and implementation; the live example used the documented local-source installation route.
- Documentation type checking and static build passed.
