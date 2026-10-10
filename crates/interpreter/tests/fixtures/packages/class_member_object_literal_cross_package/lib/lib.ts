export class Probe {
  seen: number;

  constructor() {
    this.seen = 0;
  }

  toJson(): string {
    const payload = { foo: 1 };
    return JSON.stringify(payload);
  }

  describe(): string {
    const detail = { kind: "probe", n: this.seen };
    return JSON.stringify(detail);
  }
}
