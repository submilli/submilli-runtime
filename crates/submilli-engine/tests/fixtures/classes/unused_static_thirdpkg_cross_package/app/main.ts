import { label } from "@test/uconsumer";

function main(): void {
  assert(label() === "x", "class used without its statics, across a package it cannot name");
}
