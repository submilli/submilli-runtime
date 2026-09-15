import { Mid } from "@test/glib";

class Local extends Mid {}

function main(): void {
  const l = new Local();
  assert(l instanceof Mid, "local subclass of a class whose ancestor owns class-typed statics");
  assert(Local.W.id === 1, "inherited class-typed static field");
}
