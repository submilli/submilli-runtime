// A local enum's members are values in its block, and it shadows an outer
// binding of the same name; an inner binding shadows it in turn.

const Level = 7;

function label(n: number): string {
  enum Level { Low, High }
  const level: Level = n > 5 ? Level.High : Level.Low;
  // Exhaustive: no return is missing after the switch.
  switch (level) {
    case Level.Low:
      return "low";
    case Level.High:
      return "high";
  }
}

function shadowed(): number {
  enum Level { Low = 1, High = 2 }
  const before = Level.High;
  {
    const Level = { High: 40 };
    return before + Level.High;
  }
}

function named(): string {
  enum Color { Red = "red", Blue = "blue" }
  const c: Color = Color.Blue;
  return c;
}

function main(): void {
  assert(label(9) === "high", "high");
  assert(label(1) === "low", "low");
  assert(shadowed() === 42, "inner const shadows the local enum");
  assert(Level === 7, "the top-level const is untouched");
  assert(named() === "blue", "string enum");
}
