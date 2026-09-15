export class Top {
  static readonly F: (n: number) => number = (n: number): number => n + 1;
  private static readonly H: (n: number) => number = (n: number): number => n * 2;

  static make(): (s: string) => string {
    return (s: string): string => s + "!";
  }

  static hidden(n: number): number {
    return Top.H(n);
  }
}

export class Mid extends Top {}

export class GenTop<T> {
  static readonly G: (n: number) => number = (n: number): number => n + 10;
}

export class GenMid extends GenTop<string> {}
