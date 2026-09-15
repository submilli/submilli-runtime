// The top-level counterpart of `closure_sig_from_class_members`. A function
// type that occurs *only* in a declared signature — never at a call site,
// never as a closure literal — still needs its closure sig registered, or the
// module's own signature emission panics before any body is walked.
//
// Every body throws rather than calling its callback: calling `f` would make
// `f` an expression, and the expression walk registers the sig from `expr.ty`,
// which would cover for a missing declaration walk and make the case vacuous.
//
// The declared arities are 6 / 2 / 3 / 7 / 5, none of which a member's own
// dispatch sig can produce here — each of these functions and members takes a
// single parameter, so the sigs they generate are all arity 1. `ClosureSig` is
// only `(arity, is_void)`, so a shared pair would let one shape stand in for
// another and hide a missing walk.
function uncalledParam(f: (a: number, b: number, c: number, d: number, e: number, g: number) => number): number {
  throw new Error("never called");
}

function uncalledReturn(): (a: number, b: string) => boolean {
  throw new Error("never called");
}

function uncalledVoidParam(f: (a: number, b: number, c: number) => void): void {
  throw new Error("never called");
}

class Statics {
  static apply(f: (a: number, b: number, c: number, d: number, e: number, g: number, h: number) => string): string {
    throw new Error("never called");
  }
}

class Ctor {
  private readonly f: (a: number, b: number, c: number, d: number, e: number) => boolean;

  constructor(f: (a: number, b: number, c: number, d: number, e: number) => boolean) {
    this.f = f;
  }
}

function main(): void {
  assert(1 === 1, "signature-only function types compile");
}
