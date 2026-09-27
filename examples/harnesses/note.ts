import fs from "submilli:fs";

function main(): string {
    fs.mkdir("/u_ada/notes", true);
    fs.writeText("/u_ada/notes/check.md", "# Written by a check\n");
    const names: string[] = [];
    for (const entry of fs.list("/u_ada/notes", false)) names.push(entry.name);
    return `notes: ${names.join(", ")}`;
}
