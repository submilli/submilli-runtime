import { Alpha, Mid } from "@test/zchk";
class Local extends Alpha {}
class Sub extends Mid {}
function main(): void {
  assert(Alpha.tag() === "alpha", "plain static");
  assert(Local.tag() === "alpha", "plain static via subclass");
  assert(new Sub() instanceof Mid, "ancestor-only class with a function-typed static");
}
