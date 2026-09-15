// The repo-root tsconfig.json extends .submilli/tsconfig.submilli.json, which
// `submilli build init` generates and .gitignore excludes. Rolldown (Vite's
// bundler) walks up from this package into the repo root and resolves that
// extends chain eagerly, so a checkout without .submilli fails the build.
// Create a harmless stub when the generated file is absent; never overwrite a
// real one.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const generated = resolve(repoRoot, ".submilli/tsconfig.submilli.json");

if (existsSync(generated)) {
  process.exit(0);
}

mkdirSync(dirname(generated), { recursive: true });
writeFileSync(generated, "{}\n");
console.log(`[docs] wrote stub ${generated} (submilli build init not run)`);
