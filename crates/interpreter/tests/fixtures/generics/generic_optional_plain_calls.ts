// Optional calls and union calls of plain function values still work; only
// generic function values are rejected there.
interface Box {
  cb?: (x: number) => number;
  inner?: { f?: (s: string) => string };
}

function call(f: ((x: number) => number) | undefined): number | undefined {
  return f?.(1);
}

function main(): void {
  const b: Box = { cb: (x: number): number => x + 1, inner: { f: (s: string): string => s + "!" } };
  assert(b.cb?.(1) === 2, "an optional field");
  assert(b.inner?.f?.("a") === "a!", "a chain");
  assert(call((x: number): number => x * 3) === 3, "a nullable parameter");
  assert(call(undefined) === undefined, "absent");
  const flag: boolean = true;
  const u = flag ? (x: string): number => x.length : (x: string): number => 0;
  assert(u("abc") === 3, "a union of plain functions");
}
