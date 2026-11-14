// A quoted "constructor" is the class's constructor, as in JavaScript.
class K {
  v: number;
  "constructor"(v: number) {
    this.v = v;
  }
}
function main(): void {
  assert(new K(3).v === 3);
}
