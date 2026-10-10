// Variance is measured once per generic declaration, so a chain of classes
// that each reference the next twice still compares in linear time.
class L0<T> {
  constructor(
    public a: L1<T> | null,
    public b: L1<T> | null,
  ) {}
}

class L1<T> {
  constructor(
    public a: L2<T> | null,
    public b: L2<T> | null,
  ) {}
}

class L2<T> {
  constructor(
    public a: L3<T> | null,
    public b: L3<T> | null,
  ) {}
}

class L3<T> {
  constructor(
    public a: L4<T> | null,
    public b: L4<T> | null,
  ) {}
}

class L4<T> {
  constructor(
    public a: L5<T> | null,
    public b: L5<T> | null,
  ) {}
}

class L5<T> {
  constructor(
    public a: L6<T> | null,
    public b: L6<T> | null,
  ) {}
}

class L6<T> {
  constructor(
    public a: L7<T> | null,
    public b: L7<T> | null,
  ) {}
}

class L7<T> {
  constructor(
    public a: L8<T> | null,
    public b: L8<T> | null,
  ) {}
}

class L8<T> {
  constructor(
    public a: L9<T> | null,
    public b: L9<T> | null,
  ) {}
}

class L9<T> {
  constructor(
    public a: L10<T> | null,
    public b: L10<T> | null,
  ) {}
}

class L10<T> {
  constructor(
    public a: L11<T> | null,
    public b: L11<T> | null,
  ) {}
}

class L11<T> {
  constructor(
    public a: L12<T> | null,
    public b: L12<T> | null,
  ) {}
}

class L12<T> {
  constructor(
    public a: L13<T> | null,
    public b: L13<T> | null,
  ) {}
}

class L13<T> {
  constructor(
    public a: L14<T> | null,
    public b: L14<T> | null,
  ) {}
}

class L14<T> {
  constructor(
    public a: L15<T> | null,
    public b: L15<T> | null,
  ) {}
}

class L15<T> {
  constructor(
    public a: L16<T> | null,
    public b: L16<T> | null,
  ) {}
}

class L16<T> {
  constructor(
    public a: L17<T> | null,
    public b: L17<T> | null,
  ) {}
}

class L17<T> {
  constructor(
    public a: L18<T> | null,
    public b: L18<T> | null,
  ) {}
}

class L18<T> {
  constructor(
    public a: L19<T> | null,
    public b: L19<T> | null,
  ) {}
}

class L19<T> {
  constructor(
    public a: L20<T> | null,
    public b: L20<T> | null,
  ) {}
}

class L20<T> {
  constructor(
    public a: L21<T> | null,
    public b: L21<T> | null,
  ) {}
}

class L21<T> {
  constructor(
    public a: L22<T> | null,
    public b: L22<T> | null,
  ) {}
}

class L22<T> {
  constructor(
    public a: L23<T> | null,
    public b: L23<T> | null,
  ) {}
}

class L23<T> {
  constructor(
    public a: T | null,
    public b: T | null,
  ) {}
}

function main(): void {
  const narrow = new L0<number>(null, null);
  const wide: L0<number | string> = narrow;
  assert(wide.a === null, "variance chain");
}
