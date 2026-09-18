import { Child } from "guard-lib";

export function main(): void {
  const child = new Child();
  child.reset();
  try {
    const value = child.value;
    assert(false, "the imported narrowed read must reject the parent value");
  } catch (e) {
    assert(e instanceof TypeError, "an imported narrowed read throws TypeError");
  }
}
