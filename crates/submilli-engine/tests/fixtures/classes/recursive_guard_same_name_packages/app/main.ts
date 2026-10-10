import { Node } from "pkg-a";
import { Child } from "pkg-b";

export function main(): void {
  const fromA: Node = { a: 1, next: null };
  assert(fromA.a === 1, "the first package alias is reachable");

  const child = new Child();
  assert(child.value.b === "ok", "the second package alias validates its own body");
  child.reset({ a: 1, next: null });
  try {
    const value = child.value;
    assert(false, "same-named alias from another package must not validate");
  } catch (e) {
    assert(e instanceof TypeError, "wrong same-named imported alias throws TypeError");
  }
}
