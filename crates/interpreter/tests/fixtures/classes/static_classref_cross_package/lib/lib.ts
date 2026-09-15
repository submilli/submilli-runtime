export class Zed {
  constructor(readonly tag: string) {}
}

export class Alpha {
  static readonly Z: Zed = new Zed("z");
  static readonly PAIR: [Zed, number] = [new Zed("p"), 1];
  static readonly MAYBE: Zed | null = null;

  static pick(z: Zed): Zed {
    return z;
  }

  static tagOf(z: Zed): string {
    return z.tag;
  }
}
