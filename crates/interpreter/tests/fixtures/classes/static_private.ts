class Registry {
  private static readonly KEY: number = 3;
  private static secret(): number {
    return 7;
  }
  static open(): number {
    return Registry.secret() + Registry.KEY;
  }
}

function main(): void {
  assert(Registry.open() === 10);
}
