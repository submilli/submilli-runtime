import session from "submilli:session";
import { get as namedGet } from "submilli:session";

function read(readValue: <T>(key: string) => T | undefined): number | undefined {
  return readValue<number>("number");
}

function rejects(readValue: () => unknown): boolean {
  try {
    readValue();
  } catch (e) {
    return e instanceof TypeError;
  }
  return false;
}

function main(): void {
  session.set("number", 42);
  session.set("null", null);
  session.set("nested", { items: ["wrong"] });
  const get = namedGet;
  const numberGet: (key: string) => number | undefined = namedGet;
  assert(get<number>("number") === 42, "generic alias");
  assert(numberGet("number") === 42, "concrete function value");
  assert(read(namedGet) === 42, "higher-order generic");
  assert(get("number") === 42, "uninferred type");
  assert(get<unknown>("number") === 42, "explicit unknown");
  assert(get<number>("missing") === undefined, "missing key");
  assert(get<null>("null") === null, "stored null");
  assert(rejects(() => get<number>("null")), "null mismatch");
  assert(rejects(() => numberGet("null")), "concrete alias mismatch");
  assert(rejects(() => get<{ items: number[] }>("nested")), "nested mismatch");
  assert(rejects(() => get<() => void>("number")), "function predicate");
}
