export class Maybe {
  private n: number;
  constructor(n: number) {
    this.n = n;
  }

  get(): unknown {
    if (this.n > 0) {
      return this.n;
    }
  }

  getOrNothing(): unknown {
    if (this.n < 0) {
      return;
    }
    return this.n;
  }
}

export function maybe(n: number): unknown {
  if (n < 0) {
    return;
  }
  if (n > 0) {
    return n;
  }
}
