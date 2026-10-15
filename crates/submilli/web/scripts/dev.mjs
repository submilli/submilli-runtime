// `npm run dev -- <project folder> [--restart] [--no-open]`: serves the page with live
// reload against the playground in that folder, and opens a link that signs this browser
// in (`--no-open` only prints it). With no playground running there, it starts one;
// `--restart` first rebuilds this repo's `submilli` and restarts the playground on it,
// for a change to the CLI or server.
// The page's `/api` requests go to the playground (see `vite.config.ts`).
import { execFileSync, spawn } from "node:child_process";
import { existsSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "vite";

const USAGE = "npm run dev -- <project folder> [--restart] [--no-open]";
const FLAGS = ["--restart", "--no-open"];
const args = process.argv.slice(2);
const restart = args.includes("--restart");
const noOpen = args.includes("--no-open");
const project = args.find((arg) => !FLAGS.includes(arg));
// A mistyped flag is refused rather than left out: `--restart.` would otherwise serve
// the page against the playground already running, on its old build.
const unknown = args.filter((arg) => arg !== project && !FLAGS.includes(arg));
if (!project || project.startsWith("-") || unknown.length) {
  const said = !project || project.startsWith("-") ? "Give the folder of a playground project" : "Not an option here: " + unknown.join(" ");
  console.error(said + "\nUsage: " + USAGE);
  process.exit(2);
}
// This repo's build, so the playground commands match the page being worked on.
const repo = new URL("../../../../", import.meta.url).pathname;
const bin = repo + "target/debug/submilli";
const submilli = (...command) => execFileSync(bin, ["playground", ...command], { cwd: project, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
// What a failed command said, for the line that explains it.
const said = (error) => String(error.stderr || error.stdout || error.message).trim();

if (restart) {
  console.log("Building submilli…");
  // Its output only when it fails: a good build's linker notes are noise here.
  try {
    execFileSync("cargo", ["build", "-p", "submilli"], { cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * 1024 * 1024 });
  } catch (error) {
    console.error(said(error));
    process.exit(1);
  }
  console.log("Restarting the playground…");
  try {
    submilli("restart", "--json");
  } catch (error) {
    console.error(said(error));
    process.exit(1);
  }
} else if (!existsSync(bin)) {
  console.error(`No build at ${bin}; run \`cargo build -p submilli\` first, or pass --restart.`);
  process.exit(1);
}

const open = () => JSON.parse(submilli("open", "--json"));
let ready;
try {
  ready = open();
} catch (error) {
  if (!/"running":\s*false/.test(said(error))) {
    console.error(said(error));
    process.exit(1);
  }
  // Nothing running there: start it, as `submilli playground` would.
  console.log("Starting the playground…");
  try {
    submilli("start", "--json");
    ready = open();
  } catch (failed) {
    console.error(said(failed));
    process.exit(1);
  }
}
const playground = new URL(ready.url).origin;
process.env.PLAYGROUND = playground;
const server = await createServer({ configFile: new URL("../vite.config.ts", import.meta.url).pathname });
await server.listen();
const local = server.resolvedUrls.local[0].replace(/\/$/, "");
const link = `${local}/${new URL(ready.login_url).hash}`;
const opened = !noOpen && openInBrowser(link);
console.log(`\n  Playground  ${playground}  (${project})`);
console.log(`  Open        ${link}`);
console.log(
  opened
    ? "              Opened in your browser; it signs in one browser."
    : `              Signs in one browser; open it within ${Math.round(ready.expires_in_secs / 60)} minutes.`,
);
console.log(`              Page changes show at once. For a CLI or server change, stop with Ctrl+C`);
console.log(`              and run it again with --restart.\n`);

// Opens `url` in the default browser through a page only this user can read that sends
// the browser on to it, as `submilli playground start` does: an opener's arguments show
// in every user's process list, and the login code would show with them.
function openInBrowser(url) {
  try {
    const page = JSON.parse(submilli("status", "--json")).state_dir + "/open-dev.html";
    rmSync(page, { force: true });
    writeFileSync(
      page,
      `<!doctype html><meta charset="utf-8"><meta name="referrer" content="no-referrer"><meta http-equiv="refresh" content="0;url=${url}"><title>Submilli playground</title>\n`,
      { mode: 0o600, flag: "wx" },
    );
    spawn(process.platform === "darwin" ? "open" : "xdg-open", [page], { stdio: "ignore", detached: true })
      .on("error", () => {})
      .unref();
    return true;
  } catch {
    return false;
  }
}
