import { Box } from "@test/base";

// No constructor: the signature is inherited as `(v: number)`.
export class NumBox extends Box<number> {
  twice(): number {
    return this.get() * 2;
  }
}
