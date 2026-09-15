import { Token } from "@test/p2";

export class Api {
  static readonly DEFAULT: Token = new Token("default");

  static make(tag: string): Token {
    return new Token(tag);
  }
}

export class Base {
  constructor(readonly token: Token) {}
  label(): string {
    return this.token.describe();
  }
}
