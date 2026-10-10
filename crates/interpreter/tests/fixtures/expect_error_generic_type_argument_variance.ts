// A sink whose function-typed property takes a `number` is not a sink of
// `string | number`: handed a string, its `put` would fail at run time. tsc
// rejects each assignment below, measuring each type parameter's variance.
// expect-error: expected `Sink<number | string>`, got `Sink<number>`
// expect-error: expected `Both<number | string>`, got `Both<number>`
// expect-error: expected `Both<1>`, got `Both<number>`
// expect-error: expected `Logger<string>`, got `Logger<number>`
// expect-error: expected `Handler<number | string>`, got `Handler<number>`
// expect-error-count: 5
class Sink<A> {
  constructor(public put: (a: A) => string) {}
}

class Both<T> {
  constructor(
    public v: T,
    public f: (x: T) => void,
  ) {}
}

class Logger<T> {
  log(x: T): void {}
}

interface Handler<A> {
  handle: (a: A) => string;
}

function main(): void {
  const sinkNum: Sink<number> = new Sink((n: number) => n.toFixed(1));
  const s: Sink<string | number> = sinkNum;
  const both: Both<number> = new Both<number>(1, (x: number) => {});
  const wider: Both<string | number> = both;
  const narrower: Both<1> = both;
  const logger: Logger<string> = new Logger<number>();
  const handler: Handler<number> = { handle: (n: number) => n.toFixed(1) };
  const h: Handler<string | number> = handler;
}
