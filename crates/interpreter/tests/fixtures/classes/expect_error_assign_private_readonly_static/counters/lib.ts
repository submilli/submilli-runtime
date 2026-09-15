export class Counter {
  private static readonly SECRET: number = 42;
  private static hidden(): number {
    return 1;
  }

  static read(): number {
    return Counter.SECRET + Counter.hidden();
  }
}
