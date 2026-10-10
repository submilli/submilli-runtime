// expect-error: static members are not accessible through a namespace import
import mathx from "@test/mathx";

function main(): void {
  mathx.Calc.add(1, 2);
}
