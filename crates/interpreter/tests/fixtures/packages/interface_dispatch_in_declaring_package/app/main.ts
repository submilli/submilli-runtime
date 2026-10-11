import { Handler, invoke, maybe, unrelated } from "@test/lib";

function main(): void {
  let seen = "";
  const h: Handler = {
    run: (a: string, b: string, c: string, d: string, e: string, f: string): void => {
      seen = a + f;
    },
    fetch: (a: string, b: string, c: string, d: string, e: string): string => a + e,
  };

  invoke(h);
  assert(seen === "16", "void-returning dispatch in the declaring package");
  assert(maybe(h) === "15", "optional-chain dispatch");
  assert(maybe(null) === undefined, "optional chain short-circuits");
  assert(unrelated() === 2, "unrelated shape shares the field name");
}
