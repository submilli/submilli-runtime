// A closure whose every `return` diverges returns `void` when its body can
// also run off its end, as tsc infers, and `never` when it can't: after a
// loop that only a diverging `return` leaves, or a switch that covers every
// enum member.
enum K {
  A,
  B,
}

function boom(): never {
  throw new Error("boom");
}

function pick(f: () => number): number {
  return 1;
}

function main(): void {
  const out: number[] = [];
  const key: string = ["a"][0];
  const f = () => {
    if (key === "b") {
      return boom();
    }
    out.push(1);
  };
  f();
  assert(out.length === 1, "the body ran off its end");

  let n = 0;
  const loop = () => {
    for (;;) {
      n++;
      if (n > 3) {
        return boom();
      }
    }
  };
  const spin = () => {
    while (true) {
      n++;
      if (n > 6) {
        return boom();
      }
    }
  };
  let caught = 0;
  try {
    const y: number = loop();
  } catch (e) {
    caught++;
  }
  try {
    const z: string = spin();
  } catch (e) {
    caught++;
  }
  assert(caught === 2, "a loop left only by diverging returns never ends");

  const k: K = [K.A][0];
  assert(
    pick(() => {
      switch (k) {
        case K.A:
          return boom();
        case K.B:
          return boom();
      }
    }) === 1,
    "a switch over every enum member never ends",
  );
}
