// A class may `implements` a type alias of an object type, as in TypeScript, and
// is checked against its members as against an interface's.
type Named = { name: string };
type Counter = { count: number; next(): number };
type Box<T> = { value: T };

class Person implements Named {
  name: string = "Ada";
}

class Ticker implements Counter, Named {
  count: number = 0;
  name: string = "ticker";
  next(): number {
    this.count += 1;
    return this.count;
  }
}

class Holder<T> implements Box<T> {
  constructor(public value: T) {}
}

function main(): void {
  const named: Named = new Person();
  const ticker = new Ticker();
  ticker.next();
  assert(named.name === "Ada", "an alias-implementing class is the alias's type");
  assert(ticker.next() === 2, "methods dispatch as usual");
  assert(new Holder<number>(3).value === 3, "a generic alias");
}
