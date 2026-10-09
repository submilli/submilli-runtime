class Empty {}
class DerivedEmpty extends Empty { extra: number = 1; }
class NamedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NamedError";
  }
}
function describe(value: Error | null | undefined): string | undefined {
  return value?.toString?.();
}
function describeEmpty(value: Empty | null | undefined): string | undefined {
  return value?.toString?.();
}

let receiverReads = 0;
let argumentReads = 0;
function encoder(present: boolean): TextEncoder | null {
  receiverReads += 1;
  return present ? new TextEncoder() : null;
}
function input(): string { argumentReads += 1; return "hello"; }
function encode(value: TextEncoder | null | undefined): Uint8Array | undefined {
  return value?.encode?.(input());
}
function checkCounts(receivers: number, args: number): void {
  assert(receiverReads === receivers && argumentReads === args);
}

function main(): void {
  assert(new Error("hello").toString?.() === "Error: hello");
  const named = new NamedError("hello");
  assert(named.name === "NamedError" && named.message === "hello");
  assert(describe(named) === named.toString(), "optional subclass conversion matches normal dispatch");
  assert(String(named) === named.toString(), "implicit subclass conversion matches normal dispatch");
  assert(describe(null) === undefined && describe(undefined) === undefined);
  assert(new Empty().toString?.() === "[object Object]");
  assert(describeEmpty(new DerivedEmpty()) === "[object Object]");
  assert(describeEmpty(null) === undefined && describeEmpty(undefined) === undefined);

  assert(new TextEncoder().encode?.("hello")?.length === 5);
  assert(encoder(true)?.encode?.(input())?.length === 5);
  checkCounts(1, 1);
  assert(encoder(false)?.encode?.(input()) === undefined);
  checkCounts(2, 1);
  assert(encode(undefined) === undefined && encode(null) === undefined);
  checkCounts(2, 1);
}
