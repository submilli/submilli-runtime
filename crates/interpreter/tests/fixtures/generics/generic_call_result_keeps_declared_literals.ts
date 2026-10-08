// A literal-union value read out of a generic call's result keeps its type
// in an array literal, as in tsc: the call inferred its type argument from a
// declared `Phase`, so `[m.state]` is a `Phase[]`. One the call inferred from
// a fresh literal still widens.
type Phase = "idle" | "running" | "stopped";

interface Machine<S> {
  state: S;
}

function create<S>(initial: S): Machine<S> {
  return { state: initial };
}

function ident<S>(x: S): S {
  return x;
}

function label(phase: Phase): string {
  return phase.toUpperCase();
}

function main(): void {
  const all: Phase[] = ["idle", "running"];
  const init = all[0];
  const m = create(init);
  const m2 = create(all[1]);
  const states: Phase[] = [m.state, m2.state];
  const copied = ident(init);
  const viaIdent: Phase[] = [copied, ident(init)];
  assert(states.map(label).join(",") === "IDLE,RUNNING", "a field of a generic call's result");
  assert(viaIdent.length === 2, "a generic call's result");

  const fresh = create("a");
  const widened = [fresh.state];
  widened.push("other");
  assert(widened.length === 2, "a fresh literal still widens");
}
