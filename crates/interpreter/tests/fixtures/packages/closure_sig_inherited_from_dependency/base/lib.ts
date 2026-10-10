export class Base {
  tally: number = 0;

  handle(fn: (a: number, b: number, c: number, d: number, e: number, f: number) => number): void {
    this.tally = this.tally + 1;
  }

  build(): (a: number, b: number, c: number, d: number) => string {
    throw new Error("never called");
  }

  get pipe(): (a: string, b: string, c: string, d: string, e: string, f: string, g: string) => void {
    throw new Error("never read");
  }

  set feed(f: (a: number, b: number, c: number, d: number, e: number) => string) {
    this.tally = this.tally + 1;
  }

  count(): number {
    return this.tally;
  }
}
