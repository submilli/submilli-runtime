export class Counter {
  total: number;
  label: string;
  constructor(label: string = "lib", ...ns: number[]) {
    let t = 0;
    for (const n of ns) {
      t += n;
    }
    this.total = t;
    this.label = label;
  }
  sum(bonus: number = 10, ...extra: number[]): number {
    let t = this.total + bonus;
    for (const e of extra) {
      t += e;
    }
    return t;
  }
}

export class Props {
  constructor(
    readonly name: string,
    readonly scale: number = 3,
    readonly tags: string[] = [],
  ) {}
}

export class Bag<T> {
  first: T;
  count: number;
  constructor(first: T, ...rest: T[]) {
    this.first = first;
    this.count = rest.length;
  }
}

export class Tagged<T> {
  value: T;
  tag: string;
  constructor(value: T, tag: string = "none") {
    this.value = value;
    this.tag = tag;
  }
}

// A subclass declared in the producer that omits the parent's defaulted and
// rest slots at its own `super(...)`.
export class Quiet extends Counter {
  constructor() {
    super("quiet");
  }
}

export enum Tone {
  Soft = 0,
  Loud = 1,
}

export enum Shade {
  Pale = "pale",
  Deep = "deep",
}

export class Peer {
  constructor(readonly n: number) {}
}

// Defaults whose value has to survive the declaration round-trip: the consumer
// re-resolves the enum by its mangled name, and the boxed slots (`unknown`, a
// union, `T | null`) need the literal boxed at the call site.
export class Themed {
  constructor(
    readonly tone: Tone = Tone.Loud,
    readonly shade: Shade = Shade.Deep,
    readonly peer: Peer | null = null,
    readonly any: unknown = 5,
    readonly either: number | string = 7,
  ) {}
}

// A rest slot whose elements are references, not `f64`.
export class PeerBag {
  total: number;
  label: string;
  constructor(label: string = "bag", ...peers: Peer[]) {
    let t = 0;
    for (const p of peers) {
      t += p.n;
    }
    this.total = t;
    this.label = label;
  }
}
