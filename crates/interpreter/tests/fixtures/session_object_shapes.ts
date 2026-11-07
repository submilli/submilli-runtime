import * as session from "submilli:session";
function main(): void {
 session.set("shape", 1);
 const page = session.list("shape", 10);
 const record = page as Record<string, unknown>;
 assert(record === page, "page identity");
 assert(Object.keys(page).sort().join(",") === "entries,nextCursor", "undefined cursor is present");
 assert(record["nextCursor"] === undefined, "undefined cursor value");
 assert(!JSON.stringify(page).includes("nextCursor"), "JSON omits undefined");
 const entry = page.entries[0];
 assert((entry as Record<string, unknown>)["key"] === "shape", "entry shape");
 assert(Object.keys(entry).sort().join(",") === "key,sizeBytes", "entry fields");
 const subset = entry as { key: string };
 assert(subset === entry && subset.key === "shape", "entry structural cast");
}
