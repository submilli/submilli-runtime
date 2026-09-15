export class Token {
  constructor(readonly tag: string) {}
  describe(): string {
    return `token:${this.tag}`;
  }
}
