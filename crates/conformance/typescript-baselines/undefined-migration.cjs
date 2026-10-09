// Inventory from the pinned upstream sources, including formerly excluded twins.
// No compiler run or repository edit: redirect stdout to retain the manifest.
const fs = require("fs");
const path = require("path");
const { createHash } = require("crypto");
const ts = require("typescript");
const { findCases, excludedDirectory, exclusionBeforePort } = require("./port-suite.cjs");
const { PIN } = require("./reimport-cases.cjs");

function manifest(checkout, casesDir = path.join(__dirname, "..", "typescript")) {
  const upstream = path.join(checkout, "tests/cases/conformance");
  const exclusions = new Map([...fs.readFileSync(path.join(casesDir, "EXCLUDED.md"), "utf8")
    .matchAll(/^\| `([^`]+)` \| ([^|]+) \| (.*) \|$/gm)]
    .map(([, rel, reason, detail]) => [rel, { reason, detail }]));
  const cases = [];
  for (const file of findCases(upstream).filter((file) => !file.endsWith(".d.ts"))) {
    const source = fs.readFileSync(file, "utf8");
    const rel = path.relative(upstream, file);
    const existing = path.join(casesDir, rel);
    const baseline = existing.replace(/\.ts$/, ".types");
    const reasons = affectedSyntax(source);
    if (fs.existsSync(baseline) && /\bundefined\b/.test(fs.readFileSync(baseline, "utf8"))) reasons.add("baseline-undefined");
    if (!reasons.size) continue;
    const present = fs.existsSync(existing);
    const unrelated = !present && (excludedDirectory(rel) || exclusionBeforePort(rel, source));
    const adapted = present && /^\s*\/\/.*\b(?:adapted|written)\b/im.test(fs.readFileSync(existing, "utf8").split("\n").slice(0, 6).join("\n"));
    cases.push({
      path: rel,
      reasons: [...reasons].sort(),
      action: unrelated ? "retain-unrelated-exclusion" : adapted ? "review-adaptation" : reasons.size === 1 && reasons.has("baseline-undefined") ? "refresh-baseline" : "reimport",
      present,
      triaged: fs.existsSync(existing.replace(/\.ts$/, ".triage")),
      sourceSha256: createHash("sha256").update(source).digest("hex"),
      ...(exclusions.has(rel) ? { previousExclusion: exclusions.get(rel) } : {}),
    });
  }
  return { upstream: PIN, cases };
}

function affectedSyntax(source) {
  const reasons = new Set();
  let tree;
  try {
    tree = ts.createSourceFile("case.ts", source, ts.ScriptTarget.Latest, true);
  } catch {
    // Some newer upstream parser-negative cases crash the pinned tsc parser.
    // Retain potentially relevant ones in the audit instead of hiding them.
    if (/\bundefined\b/.test(source)) {
      reasons.add("undefined");
      reasons.add("upstream-parser-recovery");
    }
    return reasons;
  }
  const visit = (node) => {
    if ((ts.isIdentifier(node) && node.text === "undefined") || node.kind === ts.SyntaxKind.UndefinedKeyword) reasons.add("undefined");
    if (ts.isStringLiteral(node) && node.text === "undefined" && ts.isBinaryExpression(node.parent)
      && (ts.isTypeOfExpression(node.parent.left) || ts.isTypeOfExpression(node.parent.right))) reasons.add("typeof-undefined");
    if (ts.isVoidExpression(node)) reasons.add("void-expression");
    if (ts.isVariableDeclaration(node) && node.type?.kind === ts.SyntaxKind.VoidKeyword) reasons.add("void-value");
    if (ts.isOptionalChain(node)) reasons.add("optional-chain");
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.QuestionQuestionToken) reasons.add("nullish-coalescing");
    if (ts.isOptionalTypeNode(node) || (ts.isNamedTupleMember(node) && node.questionToken)) reasons.add("optional-tuple");
    if ((ts.isMethodSignature(node) || ts.isMethodDeclaration(node)) && node.questionToken) reasons.add("optional-method");
    if ((ts.isPropertySignature(node) || ts.isPropertyDeclaration(node)) && node.questionToken) reasons.add("optional-property");
    if (ts.isParameter(node) && node.questionToken) reasons.add("optional-parameter");
    if (ts.isParameter(node) && node.initializer) reasons.add("parameter-default");
    if (ts.isBindingElement(node) && node.initializer) reasons.add("destructuring-default");
    ts.forEachChild(node, visit);
  };
  visit(tree);
  return reasons;
}

module.exports = { manifest, affectedSyntax };
if (require.main === module) {
  if (!process.argv[2]) throw new Error("usage: node undefined-migration.cjs <pinned TypeScript checkout>");
  console.log(JSON.stringify(manifest(path.resolve(process.argv[2])), null, 2));
}
