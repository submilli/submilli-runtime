export class Counter {
  private static count: number = 0;

  static read(): number {
    return Counter.count;
  }
}
