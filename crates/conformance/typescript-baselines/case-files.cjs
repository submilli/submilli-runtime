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

/** The case's files, or `null` when it is one file: it has fewer than two
 * `@filename` lines. Each is `{ module, fileName, text, firstLine, endLine, start,
 * end }`: `module` is the path a relative import names it by, `fileName` where
 * `tsc` sees it, and it owns lines `[firstLine, endLine)` (0-based) and offsets
 * `[start, end)` of the case. */
function splitCase(text) {
  const lines = text.split("\n");
  const starts = [];
  lines.forEach((line, index) => {
    const match = FILENAME_LINE.exec(line);
    if (match) starts.push({ index, module: modulePath(match[1].trim()) });
  });
  if (starts.length < 2) return null;
  const lineStarts = [];
  let offset = 0;
  for (const line of lines) {
    lineStarts.push(offset);
    offset += line.length + 1;
  }
  return starts.map(({ module }, file) => {
    const firstLine = file === 0 ? 0 : starts[file].index;
    const endLine = starts[file + 1]?.index ?? lines.length;
    const owns = (index) => firstLine <= index && index < endLine;
    return {
      module,
      fileName: `${VIRTUAL_ROOT}/${module}.ts`,
      text: lines.map((line, index) => (owns(index) ? line : " ".repeat(line.length))).join("\n"),
      firstLine,
      endLine,
      start: lineStarts[firstLine],
      end: endLine < lines.length ? lineStarts[endLine] : text.length,
    };
  });
}

/** The file of `files` that owns 0-based line `line`. */
function fileOwning(files, line) {
  return files.find((f) => f.firstLine <= line && line < f.endLine);
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
  const lineCount = text.split("\n").length;
  return files
    .flatMap((file) => {
      const lines = step(file.text, file.fileName).split("\n");
      if (lines.length !== lineCount) throw new Error(`a step moved lines of a multi-file case: ${fileName}`);
      return lines.slice(file.firstLine, file.endLine);
    })
    .join("\n");
}

/** Module paths named by more than one of the case's files. */
function duplicateModules(files) {
  const modules = files.map((f) => f.module);
  return [...new Set(modules.filter((m, i) => modules.indexOf(m) !== i))];
}

/** The import specifier from module `from` to module `to`: `./x`, or `../x`. */
function relativeSpecifier(from, to) {
  const relative = path.posix.relative(path.posix.dirname(`/${from}`), `/${to}`);
  return relative.startsWith("../") ? relative : `./${relative}`;
}

module.exports = { VIRTUAL_ROOT, splitCase, fileOwning, fileNames, eachFile, duplicateModules, relativeSpecifier };
