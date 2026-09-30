// `a ?? f()` with a `void` right side has no value: it calls `f` when `a` is
// null, and is usable only where the result is discarded.
let log: string[] = [];

function f(): void {
  log.push("f");
}

function n(): number {
  log.push("n");
  return 1;
}

function missing(): number | null {
  log.push("missing");
  return null;
}

function present(): string | null {
  log.push("present");
  return "s";
}

function returned(value: string | null): void {
  return value ?? f();
}

function main(): void {
  const k: string = ["a"][0];
  const absent: string | null = k === "b" ? "s" : null;

  absent ?? f();
  missing() ?? f();
  assert(log.join(",") === "f,missing,f", "null left runs the right side");

  log = [];
  present() ?? f();
  assert(log.join(",") === "present", "non-null left skips the right side");

  log = [];
  absent ?? (k === "a" ? f() : n());
  absent ?? (k !== "a" ? f() : n());
  assert(log.join(",") === "f,n", "void conditional on the right");

  log = [];
  n() ?? f();
  null ?? f();
  assert(log.join(",") === "n,f", "a left side that is never null, and one that always is");

  log = [];
  returned(null);
  returned("s");
  assert(log.join(",") === "f", "returned from a void function");
}
