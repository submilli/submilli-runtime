function double(x: number): number {
  return x * 2;
}

class Config {
  static readonly BASE: number = double(21);
  static readonly NAME: string = "cfg";
}

// Module globals and static fields initialize in source order, interleaved.
const seed: number = Config.BASE + 1;

class Derived {
  static readonly FROM_OTHER: number = Config.BASE + seed;
}

function main(): void {
  assert(Config.BASE === 42);
  assert(Config.NAME === "cfg");
  assert(seed === 43);
  assert(Derived.FROM_OTHER === 85);
}
