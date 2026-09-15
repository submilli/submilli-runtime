// A class accessor's annotation is resolved twice — once by the class
// signature pass and again by the body pass, which needs the body's generic
// scope — so the nested screens inside `resolve_type` run twice. The body
// pass discards only the replays.
//
// This pins that the screen fires on an accessor annotation at all; the
// harness matches needles with `any`, so it cannot express "once, not twice".
// expect-error: `void` cannot be an array element — it has no values
class Holder {
  private w: number = 0;
  set a(x: void[]) {
    this.w = 1;
  }
}

function main(): void {}
