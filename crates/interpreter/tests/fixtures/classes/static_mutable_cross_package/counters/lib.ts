export class Counter {
  static count: number = 0;
  static readonly LIMIT: number = 10;

  static read(): number {
    return Counter.count;
  }
}
