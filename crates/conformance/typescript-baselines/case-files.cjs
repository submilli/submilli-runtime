// The files of a multi-file case. Such a case is one `.ts` file whose
// `// @filename: <name>` lines each start a file, as upstream writes them.
// `tests/support/case_files.rs` splits a case the same way, so `tsc` and we check
// the same files.
//
// Each file's text is the whole case with every line of the other files blanked to
// spaces, so an offset, line or column in any file is the same in the case, and
// `tsc`'s positions need no mapping. Lines before the first `@filename` belong to
// the first file.
const path = require("path");

// Matched as `tsc-case.cjs`'s OPTION_LINE matches any option.
const FILENAME_LINE = /^\s*\/\/\s*@filename\s*:\s*([^\r\n]*)/i;

// Where `tsc` sees a case's files. Nothing is there on disk, so module resolution
// can't find a neighbouring case.
const VIRTUAL_ROOT = "/__case__";

/** The case's files, `{ module, fileName, text }` each, or `null` when it is one
 * file: it has fewer than two `@filename` lines. `module` is the path a relative
 * import names it by, and `fileName` where `tsc` sees it. */
function splitCase(text) {
  const lines = text.split("\n");
  const starts = [];
  lines.forEach((line, index) => {
    const match = FILENAME_LINE.exec(line);
    if (match) starts.push({ index, module: modulePath(match[1].trim()) });
  });
  if (starts.length < 2) return null;
  const owners = ownersOf(lines.length, starts);
  return starts.map(({ module }, file) => ({
    module,
    fileName: `${VIRTUAL_ROOT}/${module}.ts`,
    text: lines.map((line, index) => (owners[index] === file ? line : " ".repeat(line.length))).join("\n"),
  }));
}

/** For each line, the index of the file it belongs to. */
function ownersOf(lineCount, starts) {
  const owners = [];
  let file = 0;
  for (let index = 0; index < lineCount; index++) {
    while (file + 1 < starts.length && starts[file + 1].index <= index) file++;
    owners.push(file);
  }
  return owners;
}

function modulePath(name) {
  const bare = name.replace(/^(\.\/)+/, "").replace(/^\/+/, "");
  return bare.endsWith(".ts") ? bare.slice(0, -".ts".length) : bare;
}

/** The names of a case's files as upstream writes them, `[]` for a single file. */
function fileNames(text) {
  return text
    .split("\n")
    .map((line) => FILENAME_LINE.exec(line)?.[1].trim())
    .filter((name) => name !== undefined);
}

/** `text` with `step(fileText, fileName)` applied to each of its files, or to the
 * whole case when it is one file. `step` must keep every line where it was: the
 * result takes each line from the file it belongs to. */
function eachFile(text, fileName, step) {
  const files = splitCase(text);
  if (!files) return step(text, fileName);
  const stepped = files.map((file) => step(file.text, file.fileName).split("\n"));
  const lines = text.split("\n");
  if (stepped.some((fileLines) => fileLines.length !== lines.length)) {
    throw new Error(`a step moved lines of a multi-file case: ${fileName}`);
  }
  const owners = ownersOf(lines.length, lines.flatMap((line, index) => (FILENAME_LINE.test(line) ? [{ index }] : [])));
  return lines.map((_, index) => stepped[owners[index]][index]).join("\n");
}

/** The import specifier from module `from` to module `to`: `./x`, or `../x`. */
function relativeSpecifier(from, to) {
  const relative = path.posix.relative(path.posix.dirname(`/${from}`), `/${to}`);
  return relative.startsWith("../") ? relative : `./${relative}`;
}

module.exports = { FILENAME_LINE, VIRTUAL_ROOT, splitCase, fileNames, eachFile, ownersOf, relativeSpecifier };
