import { Calc, Pure } from "@test/mathx";

class LocalCalc extends Calc {
  static three(): number {
    return LocalCalc.add(1, 2);
  }
}

function main(): void {
  assert(Calc.add(2, 3) === 5);
  assert(Calc.ZERO === 0);
  assert(Pure.id(9) === 9);
  assert(LocalCalc.add(4, 5) === 9);
  assert(LocalCalc.ZERO === 0);
  assert(LocalCalc.three() === 3);
}
