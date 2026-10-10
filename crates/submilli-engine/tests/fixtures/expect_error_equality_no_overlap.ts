// A comparison whose operand types share no value is rejected, as tsc rejects
// it (TS2367), even under the looser comparable relation.
// expect-error: expected `NeedsB`, got `Other`
// expect-error-count: 19
interface NeedsB { b: number }
interface Other { c: number }
interface HasM { m(): number }
interface HasX { x: number }
interface Weak { optional?: boolean }
class Secret { private key: number = 1; }
class Hidden { private key: number = 1; }
class Vault { private key: number = 1; }
class Safe extends Vault { size: number = 1; }
enum Mode { On }
interface Box<T> { value: T }
interface Maybe<T> { value: T | null }
type Slot<T> = { value?: T };
class Keeper<T> { private run(value: T): T { return value; } }
interface Chain<T> { value: T; wrap(): Chain<{ w: T }> }

function instantiations(
  boxes: Box<Box<Box<Box<number>>>>,
  strings: Box<Box<Box<Box<string>>>>,
  maybe: Maybe<number>,
  other: Maybe<string>,
  numbers: Chain<number>,
  texts: Chain<string>,
  numberSlot: Slot<number>,
  textSlot: Slot<string>,
): void {
  const a = boxes === strings;
  const b = maybe === other;
  const c = new Keeper<number>() === new Keeper<string>();
  const d = numbers === texts;
  const e = numberSlot === textSlot;
}

function distinctParams<T, U>(t: T, u: U, ts: T[], us: U[]): void {
  const a = t === u;
  const b = ts === us;
  if (typeof t === "number") {
    const c = t === "a";
    switch (t) {
      case "b":
        break;
      default:
        break;
    }
  }
}

function main(): void {
  const needsB: NeedsB = { b: 1 };
  const other: Other = { c: 1 };
  const a = needsB === other;
  const nums: number[] = [1];
  const strs: string[] = ["a"];
  const b = nums === strs;
  const f: (x: number) => string = (x: number) => "";
  const g: () => number = () => 1;
  const c = f === g;
  const hm: HasM = { m: () => 1 };
  const hx: HasX = { x: 1 };
  const d = hm === hx;
  const weak: Weak = {};
  const e = weak === 1;
  const optional: { b?: number } = {};
  const mandatory: { b: string } = { b: "" };
  const h = optional === mandatory;
  const flag: boolean = true;
  const i = flag === weak;
  const j = new Secret() === new Hidden();
  const k = new Safe() === new Hidden();
  const mode: Mode = Mode.On;
  const l = mode === weak;
}
