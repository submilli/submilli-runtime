import { GenMid, Mid } from "@test/ancestry";

class Local extends Mid {}

class LocalGen extends GenMid {}

function main(): void {
  assert(new Local() instanceof Mid, "local subclass of the imported class");
  assert(Mid.F(1) === 2, "function-typed static field on the ancestor");
  assert(Local.F(2) === 3, "same static reached through the local subclass");
  assert(Mid.make()("a") === "a!", "static method returning a closure");
  assert(Mid.hidden(3) === 6, "a private function-typed static, reached through a public one");
  assert(new LocalGen() instanceof GenMid, "local subclass of the generic-ancestor class");
  assert(GenMid.G(1) === 11, "function-typed static on a generic ancestor");
  assert(LocalGen.G(2) === 12, "generic ancestor's static through the local subclass");
}
