import { mkdir, writeText, move, remove, exists, readText } from "submilli:fs";

function main(): void {
    for (const count of [64, 128]) {
        mkdir("/source/nested", true);
        for (let index = 0; index < count; index++) {
            writeText("/source/nested/file" + String(index), "kept");
        }
        move("/source", "/destination");
        assert(!exists("/source"));
        assert(readText("/destination/nested/file0") === "kept");
        remove("/destination", true);
        assert(!exists("/destination"));
    }
}
