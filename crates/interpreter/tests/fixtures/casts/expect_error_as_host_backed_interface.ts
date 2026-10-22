// A host-backed interface such as `fs.Stat` has only values the standard
// library builds: its property reads go to the host's backing struct. So `as`
// and assignment reject every source that isn't already the host type,
// wherever the interface sits in the target.
//
// expect-error-count: 11
// expect-error: `as` can't produce a `Stat`: only the standard library builds values of a host-backed interface
// expect-error: `as` can't produce a `Stat | null`
// expect-error: `as` can't produce a `{ s?: Stat }`
// expect-error: `as` can't produce a `W`
// expect-error: `as` can't produce a `{ [key: string]: Stat }`
// expect-error: `as` can't produce a `Entry`
// expect-error: `session.get<Stat | undefined>` cannot be verified at runtime: only the standard library builds values of a host-backed interface
// expect-error: expected `Stat`, got `{ kind: string; modifiedAt: number; size: number }`
// expect-error: expected `Stat`, got `S`
// expect-error: expected `Stat`, got `Mine`
import * as fs from "submilli:fs";
import * as session from "submilli:session";

interface W {
  s: fs.Stat;
}

interface Mine {
  kind: string;
  size: number;
  modifiedAt: number;
}

class S {
  kind: string = "file";
  size: number = 1;
  modifiedAt: number = 0;
}

function size(s: fs.Stat): number {
  return s.size;
}

function main(): void {
  const text = "{\"kind\":\"file\",\"size\":1,\"modifiedAt\":0}";
  const parsed = JSON.parse(text) as fs.Stat;
  const nullable = JSON.parse(text) as fs.Stat | null;
  const optional = JSON.parse(text) as { s?: fs.Stat };
  const wrapped = JSON.parse(text) as W;
  const record = JSON.parse(text) as { [k: string]: fs.Stat };
  const entry = JSON.parse(text) as session.Entry;
  const literal = { kind: "file", size: 1, modifiedAt: 0 } as fs.Stat;
  const assigned: fs.Stat = { kind: "file", size: 1, modifiedAt: 0 };
  const instance: fs.Stat = new S();
  const mine: Mine = { kind: "file", size: 1, modifiedAt: 0 };
  const viaInterface: fs.Stat = mine;
  const stored = session.get<fs.Stat>("k");
  size(parsed);
}
