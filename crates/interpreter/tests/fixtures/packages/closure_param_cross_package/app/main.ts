import { adder, announce, fold, invoke, repeat, Sink } from "@test/host";

function main(): void {
  assert(invoke((n: number) => n + 1) === 2, "closure across package boundary");

  let calls = 0;
  repeat(3, (): void => {
    calls = calls + 1;
  });
  assert(calls === 3, "void closure ran three times");

  assert(fold([1, 2, 3], 10, (acc: number, n: number) => acc + n) === 16, "fold");

  let seen = "";
  const sink: Sink = {
    emit: (msg: string): void => {
      seen = msg;
    },
  };
  announce(sink, "hi");
  assert(seen === "hi", "closure reached through an interface property");

  assert(adder(5)(2) === 7, "closure built in the package, called here");
}
