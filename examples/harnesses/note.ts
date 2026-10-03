import fs from "submilli:fs";

function main(): string {
    const before: string[] = [];
    for (const entry of fs.list(".", false)) before.push(entry.name);
    fs.writeText("check.md", "# Written by a check\n");
    return `notes before: ${before.join(", ") || "none"}`;
}
