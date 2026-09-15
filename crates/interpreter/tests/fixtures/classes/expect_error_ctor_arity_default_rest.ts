// expect-error: constructor of `Rest` expects at least 1 argument(s), got 0
// expect-error: constructor of `Defaulted` expects 1–2 argument(s), got 3
// expect-error: default value of type `string` is not assignable to parameter type `number`
// expect-error: parameter `v` cannot have a default value: its type `T` is chosen by the caller
class Rest {
  constructor(
    first: number,
    ...ns: number[]
  ) {}
}

class Defaulted {
  constructor(
    a: number,
    b: number = 1,
  ) {}
}

// A rejected middle default: the surviving defaults must keep their own slots
// rather than sliding left into it.
class Rejected {
  constructor(
    a: number = 1,
    b: number = "no",
    c: number = 3,
  ) {}
}

// A caller picks what `T` is, so no literal can stand in for it.
class Wild<T> {
  constructor(readonly v: T = 5) {}
}

function main(): void {
  const r = new Rest();
  const d = new Defaulted(1, 2, 3);
  const j = new Rejected(9);
  const w = new Wild<string>();
}
