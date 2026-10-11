const fs = require("fs");
const path = require("path");

function readCaseList(file) {
  if (!file) throw new Error("a case-list file is required");
  return parseCaseList(fs.readFileSync(file, "utf8"));
}

function parseCaseList(text) {
  const cases = [...new Set(text.split(/\r?\n/).map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#")))];
  if (!cases.length) throw new Error("case list is empty");
  for (const rel of cases) {
    if (path.isAbsolute(rel) || rel.includes("\\") || rel.split("/").some((part) => !part || part === "." || part === "..") || !rel.endsWith(".ts") || rel.endsWith(".d.ts")) {
      throw new Error(`invalid case path: ${rel}`);
    }
  }
  return cases.sort();
}

module.exports = { readCaseList, parseCaseList };
