import { guarded } from "@test/inner";

/** Calls the inner package, so the inner check must name this package. */
export function relay(foo: number): void {
  guarded(foo);
}
