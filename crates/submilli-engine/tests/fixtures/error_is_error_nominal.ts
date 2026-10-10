class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class ParseError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ParseError";
  }
}

class DeepError extends ParseError {
  constructor(message: string) {
    super(message);
    this.name = "DeepError";
  }
}

function main(): void {
  const p: unknown = new Point(1, 2);
  assert(!Error.isError(p), "a field-only class instance is not an error");

  const sub: unknown = new ParseError("bad input");
  assert(Error.isError(sub), "an Error subclass instance is an error");

  const deep: unknown = new DeepError("nested");
  assert(Error.isError(deep), "a second-level Error subclass instance is an error");

  const arr: unknown = [1, 2, 3];
  assert(!Error.isError(arr), "an array is not an error");

  const f: unknown = (n: number): number => n + 1;
  assert(!Error.isError(f), "a closure is not an error");
}
