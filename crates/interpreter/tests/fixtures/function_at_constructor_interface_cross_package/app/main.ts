import {
  Empty,
  ErasedBase,
  Formatter,
  NarrowedChild,
  customBool,
  makeFormatter,
} from "@test/callables";

function main(): void {
  assert(customBool("yes") === true && customBool(1) === false && customBool() === false);
  assert(Boolean(1) === true && Boolean(0) === false);

  const fmtNum: Formatter<number> = makeFormatter<number>("n");
  const optFmt: Formatter<number> | undefined = fmtNum;
  assert(fmtNum(42) === "n:42" && optFmt?.(7) === "n:7");

  const localFmt: Formatter<string> = (s) => s.toUpperCase();
  assert(localFmt("ok") === "OK" && typeof localFmt === "function");

  const child = new NarrowedChild();
  const base: ErasedBase = child;
  assert(child.boolSlot("yes") === true && child.boolSlot(1) === false);
  base.boolSlot = Boolean;
  assert(child.boolSlot(1) === true && child.boolSlot(0) === false);

  base.slot = (x: number): number => x + 1;
  const heldEmpty: Empty = child.slot;
  assert(typeof heldEmpty === "function" && JSON.stringify(heldEmpty) === undefined);

  base.boolSlot = (a: number, b: number): boolean => a === b;
  let badCarrierThrew = false;
  try {
    const _bad = child.boolSlot;
  } catch (e) {
    badCarrierThrew = e instanceof TypeError;
  }
  assert(badCarrierThrew);
}
