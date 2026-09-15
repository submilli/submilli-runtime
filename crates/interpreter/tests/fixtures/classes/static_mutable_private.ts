class Registry {
  private static count: number = 0;

  static bump(): number {
    Registry.count += 1;
    return Registry.count;
  }
}

function main(): void {
  // Private statics are module-scoped: the declaring module may write them.
  Registry.count = 10;
  assert(Registry.bump() === 11);
}
