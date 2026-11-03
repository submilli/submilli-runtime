function render(value: number | boolean): string {
  return value.toString();
}

function unwrap(value: number | boolean): number | boolean {
  return value.valueOf();
}

function guarded(value: unknown): string {
  if (typeof value === "number" || typeof value === "boolean") {
    return " " + value.toString();
  }
  return "other";
}

let effects = "";
function receiver(value: number | boolean | string | bigint): number | boolean | string | bigint {
  effects += "receiver;";
  return value;
}

function radix(): number {
  effects += "radix;";
  return 16;
}

function all(value: number | boolean | string | bigint, expected: string): void {
  effects = "";
  assert(receiver(value).toString(radix()) === expected);
  assert(effects === "receiver;radix;");
  assert(value.valueOf() === value);
}

function main(): void {
  assert(render(42) === "42");
  assert(render(false) === "false");
  assert(unwrap(42) === 42);
  assert(unwrap(false) === false);
  assert(guarded(42) === " 42");
  assert(guarded(false) === " false");
  assert(guarded("hello") === "other");
  all(42, "2a");
  all(true, "true");
  all("\ud800", "\ud800");
  all(42n, "2a");
  assert((42).valueOf() === 42);
  assert(false.valueOf() === false);
  assert("\ud800".valueOf().charCodeAt(0) === 0xd800);
  assert(42n.valueOf() === 42n);
  assert(Object.is(unwrap(-0), -0));
}
