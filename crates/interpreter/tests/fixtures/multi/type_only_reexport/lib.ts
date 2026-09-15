// `type as t` imports the binding named `type`; `type Shape` is the inline
// type-only modifier, accepted as a no-op synonym.
import { makeShape, type as t, type Shape } from "./util";

export type { Shape } from "./util";

export function call(): number {
  const s: Shape = makeShape(2);
  assert(s.kind === "box");
  return s.size + t;
}
