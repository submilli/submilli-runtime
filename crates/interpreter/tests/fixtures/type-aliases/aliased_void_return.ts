// `type V = void` is `void`. Every gate that decides whether a return type
// occupies a value slot has to peel, or the alias walks past the gate into a
// backend entitled to assume `void` never reaches a value slot — the function
// body below that only `throw`s reaches codegen with no diagnostic in front of
// it, and a class method skips the falls-off-the-end check entirely.
type V = void;
type V2 = V;

function fallsOffTheEnd(): V {
  console.log("no return statement at all");
}

function bareReturn(n: number): V {
  if (n > 0) {
    return;
  }
  console.log("negative");
}

function alwaysThrows(): V {
  throw new Error("boom");
}

function callsAVoidValue(f: () => V): number {
  f();
  return 1;
}

class C {
  calls: number = 0;
  m(): V {
    this.calls = this.calls + 1;
  }
  static s(): V2 {
    console.log("static");
  }
}

interface Sink {
  accept(n: number): V;
}

// An aliased-void closure and a bare-void one share a Wasm signature, so each
// has to be accepted in the other's slot — the ABI is chosen from the peeled
// return type, not the spelling.
type FV = () => V;
type FW = () => void;

function callBoth(aliased: FV, bare: FW): number {
  aliased();
  bare();
  return 2;
}

class Collector implements Sink {
  total: number = 0;
  accept(n: number): V {
    this.total = this.total + n;
  }
}

function main(): void {
  const arrow = (): V => {
    console.log("arrow body falls off the end");
  };
  const arrowTwice: () => V2 = (): V2 => {
    return;
  };

  fallsOffTheEnd();
  bareReturn(1);
  bareReturn(-1);
  arrow();
  arrowTwice();
  assert(callsAVoidValue(arrow) === 1, "an aliased-void closure is callable");

  const c: C = new C();
  c.m();
  c.m();
  assert(c.calls === 2, "aliased-void method ran twice");
  C.s();

  const collector: Collector = new Collector();
  const bare = (): void => {
    console.log("bare-void arrow");
  };
  assert(callBoth(arrow, bare) === 2, "aliased and bare void closures side by side");
  const asBare: FW = arrow;
  const asAliased: FV = bare;
  asBare();
  asAliased();

  const sink: Sink = collector;
  sink.accept(2);
  sink.accept(3);
  assert(collector.total === 5, "aliased-void through an interface");

  let threw: boolean = false;
  try {
    alwaysThrows();
  } catch (e) {
    threw = true;
  }
  assert(threw, "a throwing aliased-void function still throws");
}
