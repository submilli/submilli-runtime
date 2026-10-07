// expect-error: parameter requires a type annotation
// A modifier followed by a line break doesn't modify the parameter after it, as
// in TypeScript, so `readonly` here is a parameter name of its own.
class Point {
  constructor(
    readonly
    x: number,
  ) {}
}

function main(): void {}
