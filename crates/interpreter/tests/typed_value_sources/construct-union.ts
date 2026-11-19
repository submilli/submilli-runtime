function make(c: typeof TextEncoder | typeof TextDecoder): number {
  const v: unknown = new c();
  return typeof v === "object" && v !== null ? 1 : 0;
}

function makeWithPattern(c: typeof RegExp | typeof TextEncoder): string {
  return String(new c("x"));
}

let spreadCalls = 0;

function empty(): [] {
  spreadCalls += 1;
  return [];
}

function makeWithSpread(c: typeof TextEncoder | typeof TextDecoder): unknown {
  return new c(...empty());
}

function makeBytes(c: unknown, size: () => number): Uint8Array {
  return new (c as typeof Uint8Array)(size());
}

function rejects(c: unknown): number {
  let calls = 0;
  try {
    makeBytes(c, () => {
      calls += 1;
      return calls;
    });
  } catch (e) {
    return e instanceof TypeError ? calls : -1;
  }
  return -2;
}

class Base {}
class Child extends Base {}

function main(): number {
  if (make(TextEncoder) !== 1) return -1;
  if (make(TextDecoder) !== 1) return -2;
  if (rejects(null) !== 1) return -3;
  if (rejects(() => 0) !== 1) return -4;
  if (rejects(BigInt) !== 1) return -5;
  if (makeWithPattern(TextEncoder) === "/x/") return -6;
  if (makeWithPattern(RegExp) !== "/x/") return -7;
  makeWithSpread(TextEncoder);
  if (spreadCalls !== 1) return -8;
  if (makeBytes(Uint8Array, () => 3).length !== 3) return -9;
  if (rejects(new Child()) !== 1) return -10;
  return makeBytes(Object.assign(Uint8Array, { tag: 1 }), () => 3).length;
}
