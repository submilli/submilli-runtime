export interface Container {
  get(): number;
  tag(): string;
}

export class Base implements Container {
  constructor(private n: number) {}
  get(): number {
    return this.n;
  }
  tag(): string {
    return "base";
  }
}
