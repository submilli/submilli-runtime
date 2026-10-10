export class Base {
  private held: number = 4;
  private get secret(): number { return 99; }
  get value(): number { return this.held; }
}
