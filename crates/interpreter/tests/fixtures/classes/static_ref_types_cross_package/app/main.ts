import { Consts } from "@test/strx";

class Local extends Consts {}

function main(): void {
  assert(Consts.NAME === "hi");
  assert(Consts.XS.length === 3);
  assert(Consts.XS[2] === 3);
  assert(Consts.PAIR[0] === "a");
  assert(Consts.PAIR[1] === 1);
  assert(Consts.BIG === 42n);
  assert(Consts.OK);
  assert(Consts.COUNT === 7);

  // resolved through a local subclass of the imported class
  assert(Local.NAME === "hi");
  assert(Local.XS.length === 3);
}
