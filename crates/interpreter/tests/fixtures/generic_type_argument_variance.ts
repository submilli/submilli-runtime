// A generic class or interface relates its instantiations at each type
// parameter's variance, measured from where the parameter appears, as in tsc:
// a property or a method's return is covariant, a function-typed property's
// parameter contravariant, and a method's own parameter bivariant.
class Box<T> {
  constructor(public v: T) {}
}

class Sink<A> {
  constructor(public put: (a: A) => string) {}
}

class Logger<T> {
  log(x: T): string {
    return "logged";
  }
}

class Feed<T> {
  constructor(public each: (cb: (x: T) => void) => void) {}
}

interface Handler<A> {
  handle: (a: A) => string;
}

function both<T>(a: Sink<T>, b: Sink<T>): Sink<T> {
  return a;
}

function main(): void {
  const box: Box<number | string> = new Box<number>(1);
  assert(box.v === 1, "a property is covariant");

  const wide: Sink<string | number> = new Sink((x: string | number) => String(x));
  const narrow: Sink<number> = wide;
  assert(narrow.put(2) === "2", "a function-typed property's parameter is contravariant");
  const literal: Sink<1> = narrow;
  assert(literal.put(1) === "1", "a narrower argument takes the wider sink");

  const logger: Logger<number | string> = new Logger<number>();
  const one: Logger<1> = new Logger<number>();
  assert(logger.log(3) === "logged" && one.log(1) === "logged", "a method parameter is bivariant");

  const feed: Feed<number | string> = new Feed<number>((cb: (x: number) => void) => cb(4));
  let seen = "";
  feed.each((x: number | string) => {
    seen = String(x);
  });
  assert(seen === "4", "a callback's own parameter flips back to covariant");

  const handler: Handler<string | number> = { handle: (x: string | number) => String(x) };
  const numbers: Handler<number> = handler;
  assert(numbers.handle(5) === "5", "an interface's function-typed field is contravariant");

  const sinkNum: Sink<number> = new Sink((n: number) => n.toFixed(1));
  const either = both(sinkNum, wide);
  assert(either.put(6) === "6.0", "a wider sink fits a type parameter bound by a narrower one");
}
