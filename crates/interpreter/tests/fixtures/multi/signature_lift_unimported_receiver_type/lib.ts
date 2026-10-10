// The receiver's class is reachable through the factory's return type but is not
// imported by name, so the signature lift can only substitute by resolving the
// declaration through the FQN registry rather than this module's import scope.
// expect-error: Box<string>.put(x: string): void

// The inherited cases need both halves at once: the registry to find the
// *receiver*, and the inheritance walk to find the class that *declares* the
// method — whose parameter names the receiver never mentions (`StringBox`) or
// binds in another order (`Flip`).
// expect-error: StringBox.put(x: string): void
// expect-error: Flip<string, number>.set(k: number, v: string): void

import { makeBox, makeStringBox, makeFlip } from "./widget";

export function useBox(): void {
    const b = makeBox();
    const bad = b.put;

    const s = makeStringBox();
    const bad2 = s.put;

    const f = makeFlip();
    const bad3 = f.set;
}
