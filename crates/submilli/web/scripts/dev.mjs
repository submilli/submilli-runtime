// `npm run dev -- <project folder>`: serves the page with live reload against the
// playground running in that folder, and prints a link that signs this browser in.
// The page's `/api` requests go to the playground (see `vite.config.ts`).
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "vite";

const project = process.argv[2];
if (!project) {
  console.error("Give the folder of a running playground: npm run dev -- <project folder>");
  process.exit(2);
}
// This repo's build, so the playground commands match the page being worked on.
const bin = new URL("../../../../target/debug/submilli", import.meta.url).pathname;
if (!existsSync(bin)) {
  console.error(`No build at ${bin}; run \`cargo build -p submilli\` first.`);
  process.exit(1);
}
let ready;
try {
  const out = execFileSync(bin, ["playground", "open", "--json"], { cwd: project, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  ready = JSON.parse(out);
} catch (error) {
  // No playground running there, most likely; `open` says why.
  const said = String(error.stderr || error.stdout || error.message).trim();
  const idle = /"running":\s*false/.test(said);
  console.error((idle ? `No playground is running in ${project}.` : said) + `\nStart one there first: cd ${project} && submilli playground start`);
  process.exit(1);
}
const playground = new URL(ready.url).origin;
process.env.PLAYGROUND = playground;
const server = await createServer({ configFile: new URL("../vite.config.ts", import.meta.url).pathname });
await server.listen();
const local = server.resolvedUrls.local[0].replace(/\/$/, "");
console.log(`\n  Playground  ${playground}  (${project})`);
console.log(`  Open        ${local}/${new URL(ready.login_url).hash}`);
console.log(`              Signs in one browser; open it within ${Math.round(ready.expires_in_secs / 60)} minutes.`);
console.log(`              Page changes show at once. Stop with Ctrl+C.\n`);
