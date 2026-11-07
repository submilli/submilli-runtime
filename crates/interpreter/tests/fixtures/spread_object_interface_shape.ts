class Plain {
  x: number = 1;

  toString(): string {
    return "plain";
  }

  toJson(): string {
    return "{}";
  }
}

function main(): void {
  const value: Object = new Plain();
  const copy = { ...value };
  assert(Object.keys(copy).includes("x"), "shape-backed interface copies own fields");
}
