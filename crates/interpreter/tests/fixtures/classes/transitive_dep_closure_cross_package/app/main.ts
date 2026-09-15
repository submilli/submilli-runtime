import { viaMid, viaSub } from "@test/p3";

function main(): void {
  assert(viaMid() === "token:m|token:default", "static method and field typed by a transitive dependency");
  assert(viaSub() === "token:s", "class extended across a package that doesn't declare the ancestor's types");
}
