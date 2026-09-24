import { writeText, readText, mkdir } from "submilli:fs";
import { read, search, glob, tree, edit, insertAt, diffText, diffFiles, applyPatch } from "submilli:code";
export function main(): void {
    mkdir("src", false);
    writeText("src/a.ts", "first\nsecond\nfirst\n");
    writeText("src/b.ts", "first\nchanged\nfirst\n");
    const window = read("src/a.ts", 2, 1);
    assert(window.path === "/src/a.ts", "normalized path");
    assert(window.lines.length === 1 && window.lines[0].line === 2, "numbered window");
    assert(window.lines[0].text === "second" && window.truncated, "window text");
    const found = search("^first$", { path: "src", context: 1 });
    assert(found.matches.length === 4, "search count");
    assert(found.matches[0].after[0].text === "second", "search context");
    assert(search("second", {mode: "files"}).files[0] === "/src/a.ts", "files mode");
    assert(search("first", {mode: "counts"}).counts[0].count === 2, "counts mode");
    assert(glob("src/*.ts").entries.length === 2, "glob");
    assert(tree("src").entries.length === 2, "tree");
    const rejected = edit("src/a.ts", "first", "new");
    assert(!rejected.success && rejected.diagnostics.length === 2, "ambiguous edit");
    assert(edit("src/a.ts", "first", "last", false, 3).success, "nearLine");
    assert(insertAt("src/a.ts", 2, "inserted\n").changed, "insertion");
    const patch = diffFiles("src/a.ts", "src/b.ts");
    assert(applyPatch("src/a.ts", patch).success, "patch");
    assert(readText("src/a.ts") === readText("src/b.ts"), "roundtrip");
    assert(diffText("same", "same") === "", "empty diff");
    assert(read("src/a.ts", 99).lines.length === 0, "past EOF");
}
