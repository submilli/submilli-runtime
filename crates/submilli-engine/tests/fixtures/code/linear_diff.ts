import { diffText, insertAt } from "submilli:code";
import { writeText, readText } from "submilli:fs";

function main(): void {
    for (const count of [4096, 8192]) {
        const old = "x\n".repeat(count);
        const replacement = "y\n" + old.slice(2);
        const patch = diffText(old, replacement);
        assert(patch.includes("-x\n+y\n"));
        writeText("/large.txt", old);
        assert(insertAt("/large.txt", 1, "y\n").changed);
        assert(readText("/large.txt") === "y\n" + old);
    }
}
