// A value a `submilli:` function returns stringifies through its properties,
// as a plain object with those fields would. A resource such as a `FileWriter`
// has none and writes `{}`.
import * as fs from "submilli:fs";
import * as url from "submilli:url";
import session from "submilli:session";

function main(): void {
  fs.writeText("/a.txt", "hi");
  const stat = fs.stat("/a.txt")!;
  assert(
    JSON.stringify(stat) === `{"kind":"file","size":2,"modifiedAt":${stat.modifiedAt}}`,
    JSON.stringify(stat)!,
  );
  const back = JSON.parse(JSON.stringify(stat)!) as { kind: string; size: number; modifiedAt: number };
  assert(back.kind === stat.kind && back.size === stat.size && back.modifiedAt === stat.modifiedAt, "round trip");
  assert(
    JSON.stringify(fs.peek("/a.txt")) === `{"preview":"hi","encoding":"utf-8","lineEnding":"lf","size":2}`,
    "Peek",
  );
  const entries: string[] = [];
  for (const entry of fs.list("/", false)) entries.push(JSON.stringify(entry)!);
  assert(entries.includes(`{"kind":"file","name":"a.txt","path":"/a.txt","size":2}`), entries.join());
  const info = JSON.parse(JSON.stringify(fs.info())!) as { mode: string; sizeLimit: number; mounts: unknown[] };
  assert(info.mode === fs.info().mode && info.sizeLimit === fs.info().sizeLimit, "Info");
  assert(info.mounts.length === 0, "mounts");
  const writer = fs.writer("/b.txt");
  assert(JSON.stringify(writer) === "{}", "a resource has no data");
  writer.close();

  const u = url.parse("https://a.example:8080/x?q=1#f");
  assert(
    JSON.stringify(u) ===
      `{"protocol":"https","host":"a.example","port":8080,"path":"/x","query":{"q":"1"},"fragment":"f"}`,
    JSON.stringify(u)!,
  );
  // An absent optional property is left out, as `undefined` is.
  const bare = JSON.stringify(url.parse("https://a.example/"))!;
  assert(bare === `{"protocol":"https","host":"a.example","path":"/","query":{}}`, bare);

  session.set("k", 1);
  const page = session.list("", 10);
  assert(JSON.stringify(page) === `{"entries":[${JSON.stringify(page.entries[0])}]}`, JSON.stringify(page)!);
  assert(JSON.stringify(page.entries[0]) === `{"key":"k","sizeBytes":${page.entries[0].sizeBytes}}`, "Entry");
  // Nested in plain data, through `unknown` too.
  const held: unknown = { at: [stat] };
  assert(JSON.stringify(held) === `{"at":[${JSON.stringify(stat)}]}`, "nested");
}
