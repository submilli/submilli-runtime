export class Payload {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  value(): number {
    return this.n;
  }
}

export class BigPayload extends Payload {
  constructor(n: number) {
    super(n * 10);
  }
}

export class Holder {
  p: Payload;
  tag: string;
  seen: number;
  constructor(p: Payload, tag: string) {
    this.p = p;
    this.tag = tag;
    // Reads the class-typed param at its declared type inside the ctor body.
    this.seen = p.value();
  }
  total(): number {
    return this.p.n;
  }
}

export class TaggedHolder extends Holder {
  constructor(p: Payload) {
    super(p, "tagged");
  }
}

export class PropHolder {
  constructor(
    readonly p: Payload,
    readonly maybe: Payload | null,
  ) {}
}

// Declared before the class it takes: reconstruction order follows `extends`,
// so the ctor signature must not depend on which class is rebuilt first.
export class Early {
  v: number;
  constructor(l: Late) {
    this.v = l.k;
  }
}

export class Late {
  k: number = 8;
}

export class MaybeHolder {
  n: number;
  constructor(p: Payload | null) {
    this.n = p === null ? -1 : p.n;
  }
}

export class Marker {
  m: number = 100;
}

// A non-nullable class union: erased like a bare class type, but the shadow
// local rebinds it through the union cast rather than a plain class cast.
export class EitherHolder {
  n: number;
  constructor(e: Payload | Marker) {
    this.n = e instanceof Payload ? e.n : e.m;
  }
}

// A generic class whose constructor mixes an erased `T` with a concrete
// class-typed param — the two erase for different reasons and must still line
// up slot for slot across the boundary.
export class Box<T> {
  value: T;
  tag: Payload;
  constructor(value: T, tag: Payload) {
    this.value = value;
    this.tag = tag;
  }
  tagged(): number {
    return this.tag.n;
  }
}

export class PayloadBox extends Box<Payload> {}
