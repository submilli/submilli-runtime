// expect-error: must be `Error`
class Plain {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}

function main(): void {
  try {
    throw new Error("x");
  } catch (e: Plain) {
    assert(false);
  }
}
