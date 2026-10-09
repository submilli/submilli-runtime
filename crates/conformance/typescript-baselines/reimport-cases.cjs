// Stages selected upstream cases without running obsolete committed expectations.
// Applying a reviewed stage preserves hand-written triage and checks that no
// collaborator changed any original file since staging.
const fs = require("fs");
const path = require("path");
const { createHash } = require("crypto");
const { execFileSync } = require("child_process");
const { readCaseList } = require("./case-list.cjs");
const { port } = require("./port-case.cjs");
const { writeBaselines } = require("./write-baselines.cjs");
const { trimSourcePadding } = require("./source-padding.cjs");

const CASES = path.join(__dirname, "..", "typescript");
const SUFFIXES = [".ts", ".types", ".errors.txt", ".divergences", ".triage"];
const GENERATED = [".ts", ".types", ".errors.txt"];
const PIN = "5848bc5157b22ff7f4e3369f4645a514a433b15f";

function main(args) {
  const option = (flag) => {
    const index = args.indexOf(flag);
    if (index < 0) return undefined;
    if (!args[index + 1] || args[index + 1].startsWith("--")) throw new Error(`${flag} needs a value`);
    const value = args[index + 1];
    args.splice(index, 2);
    return value;
  };
  const list = option("--case-list");
  const apply = option("--apply-stage");
  const stage = option("--stage-dir");
  if (apply && !stage && !args.length) {
    return applyStage(path.resolve(apply), list ? readCaseList(list) : undefined);
  }
  if (!apply && stage && list && args.length >= 1 && args.length <= 2 && !args.some((arg) => arg.startsWith("--"))) {
    return stageCases(path.resolve(args[0]), readCaseList(list), path.resolve(stage), args[1] && path.resolve(args[1]));
  }
  throw new Error("usage: port-suite.cjs --case-list FILE --stage-dir DIR <TypeScript checkout> [typescript_case_errors]\n       port-suite.cjs --apply-stage DIR [--case-list REVIEWED_CASES]");
}

function stageCases(checkout, cases, stage, caseErrors, casesDir = CASES) {
  if (stage === casesDir || stage.startsWith(casesDir + path.sep)) throw new Error("stage must be outside the committed cases directory");
  const upstream = path.join(checkout, "tests", "cases", "conformance");
  const sources = cases.map((rel) => [rel, fs.readFileSync(path.join(upstream, rel), "utf8")]);
  fs.mkdirSync(stage, { recursive: false });
  const manifest = { upstream: PIN, checkout, cases: [], failed: [] };
  for (const [rel, source] of sources) {
    try {
      const entry = stageCase(upstream, rel, source, stage, caseErrors, casesDir);
      manifest.cases.push(entry);
      console.log(`staged ${rel}${entry.original[".triage"] ? " (review preserved triage)" : ""}`);
    } catch (error) {
      manifest.failed.push({ rel, message: error.message });
      console.error(`could not stage ${rel}: ${error.message}`);
    }
    fs.writeFileSync(path.join(stage, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  }
  fs.writeFileSync(path.join(stage, "cases.txt"), manifest.cases.map(({ rel }) => rel).join("\n") + "\n");
  console.log(`Review ${stage}/cases against ${stage}/original before --apply-stage. No committed expectations were run or removed.`);
  if (manifest.failed.length) throw new Error(`${manifest.failed.length} cases could not be staged; see manifest.json`);
}

function stageCase(upstream, rel, source, stage, caseErrors, casesDir) {
  const base = rel.slice(0, -3);
  const original = {};
  for (const suffix of SUFFIXES) {
    const name = base + suffix;
    const from = path.join(casesDir, name);
    original[suffix] = digest(from);
    if (fs.existsSync(from)) copy(from, path.join(stage, "original", name));
  }
  const to = path.join(stage, "cases", rel);
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.writeFileSync(to, port(source, path.join(upstream, rel)));
  let pruning;
  if (caseErrors) {
    pruning = JSON.parse(execFileSync(process.execPath, [path.join(__dirname, "prune-case.cjs"), caseErrors, to], { encoding: "utf8", maxBuffer: 1 << 26 }));
    if (pruning.error) throw new Error(`${rel}: ${pruning.error}; original case is untouched`);
  }
  fs.writeFileSync(to, trimSourcePadding(fs.readFileSync(to, "utf8")));
  writeBaselines(to);
  return { rel, original, pruning };
}

function applyStage(stage, selected, casesDir = CASES) {
  const manifest = JSON.parse(fs.readFileSync(path.join(stage, "manifest.json"), "utf8"));
  const staged = new Map(manifest.cases.map((entry) => [entry.rel, entry]));
  const cases = selected ?? readCaseList(path.join(stage, "cases.txt"));
  for (const rel of cases) {
    const entry = staged.get(rel);
    if (!entry) throw new Error(`${rel} is not staged`);
    const base = rel.slice(0, -3);
    for (const suffix of SUFFIXES) {
      if (digest(path.join(casesDir, base + suffix)) !== entry.original[suffix]) {
        throw new Error(`${rel}${suffix}: original changed after staging; re-stage before applying`);
      }
    }
    for (const suffix of [".ts", ".types"]) {
      if (!fs.existsSync(path.join(stage, "cases", base + suffix))) throw new Error(`${rel}: staged ${suffix} is missing`);
    }
  }
  for (const rel of cases) {
    const base = rel.slice(0, -3);
    for (const suffix of GENERATED) {
      const from = path.join(stage, "cases", base + suffix);
      const to = path.join(casesDir, base + suffix);
      if (fs.existsSync(from)) copy(from, to);
      else fs.rmSync(to, { force: true });
    }
  }
  console.log(`applied ${cases.length} reviewed cases; triage and divergences preserved for focused reconciliation`);
}

function digest(file) {
  return fs.existsSync(file) ? createHash("sha256").update(fs.readFileSync(file)).digest("hex") : null;
}

function copy(from, to) {
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.copyFileSync(from, to);
}

module.exports = { main, stageCases, applyStage, PIN };
