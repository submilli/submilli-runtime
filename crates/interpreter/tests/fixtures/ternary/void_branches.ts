// A conditional whose branch is a `void` call has no value. It runs one branch
// for its effects wherever an expression is evaluated and discarded: an
// expression statement, a `for` update, a `void` function's `return`, and an
// arrow body.
let log: string[] = [];

function f(): void {
  log.push("f");
}

function g(): void {
  log.push("g");
}

function n(): number {
  log.push("n");
  return 1;
}

function fails(): never {
  log.push("fails");
  throw new Error("fails");
}

class Sink {
  emit(): void {
    log.push("emit");
  }
}

function returned(k: string): void {
  return k === "a" ? f() : g();
}

function narrowed(k: string | null): void {
  k !== null ? log.push(k) : g();
  k === null ? f() : log.push(k);
}

type Shape = { kind: "a"; go: () => void } | { kind: "b"; n: number };

function discriminated(shape: Shape): void {
  shape.kind === "a" ? shape.go() : log.push(shape.n.toString());
  shape.kind === "b" ? f() : shape.go();
}

function taken(): string {
  const seen = log.join(",");
  log = [];
  return seen;
}

function main(): void {
  const k: string = ["a"][0];
  const sink: Sink | null = k === "a" ? new Sink() : null;

  k === "a" ? f() : g();
  k !== "a" ? f() : g();
  assert(taken() === "f,g", "both branches void");

  k === "a" ? f() : n();
  k !== "a" ? f() : n();
  k === "a" ? n() : g();
  assert(taken() === "f,n,n", "a value branch is evaluated and discarded");

  k === "a" ? (sink === null ? f() : g()) : f();
  (k === "a" ? f() : g());
  assert(taken() === "g,f", "nested and parenthesized");

  k === "a" ? sink?.emit() : g();
  k !== "a" ? f() : sink!.emit();
  assert(taken() === "emit,emit", "void method branches");

  for (let i = 0; i < 2; k === "a" ? f() : g()) {
    i++;
  }
  assert(taken() === "f,f", "for update");

  returned("a");
  returned("b");
  assert(taken() === "f,g", "returned from a void function");

  narrowed("x");
  narrowed(null);
  assert(taken() === "x,x,g,f", "branches under a narrowing");

  discriminated({ kind: "a", go: (): void => g() });
  discriminated({ kind: "b", n: 7 });
  assert(taken() === "g,g,7,f", "branches under a discriminated union");

  for (k === "a" ? f() : g(); log.length < 2; ) {
    log.push("body");
  }
  assert(taken() === "f,body", "for init");

  const annotated = (): void => (k === "a" ? f() : g());
  const inferred = () => (k !== "a" ? f() : n());
  annotated();
  inferred();
  [1, 2].forEach((x: number) => (x === 1 ? f() : g()));
  assert(taken() === "f,n,f,g", "arrow bodies");

  try {
    k === "a" ? fails() : f();
  } catch (e) {
    log.push("caught");
  }
  k === "b" ? fails() : f();
  assert(taken() === "fails,caught,f", "a branch that never returns");
}
