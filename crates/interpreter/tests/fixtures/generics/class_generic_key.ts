class C {
  x: number = 1;
  y: string = "s";
  private secret: number = 5;
  get(): number { return this.x; }
}
function read<K extends keyof C>(c: C, k: K): C[K] { return c[k]; }
function write<K extends "x" | "y">(c: C, k: K, v: C[K]): void { c[k] = v; }
function writeAny<K extends keyof C>(c: C, k: K, value: C[K]): void { c[k] = value; }
function maybe<K extends "x" | "y">(c: C | undefined, k: K): C[K] | undefined { return c?.[k]; }
function main(): void {
  const c = new C();
  assert(typeof read(c, "get") === "function");
  // Class method slots remain immutable in Submilli (unlike JavaScript).
  let refused = false;
  try { writeAny(c, "get", () => 4); } catch (error) { refused = error instanceof TypeError; }
  assert(refused);
  assert(read(c, "x") === 1 && read(c, "y") === "s");
  write(c, "x", 7);
  write(c, "y", "ok");
  assert(read(c, "x") === 7 && read(c, "y") === "ok");
  assert(maybe(c, "x") === 7 && maybe(undefined, "y") === undefined);
}
