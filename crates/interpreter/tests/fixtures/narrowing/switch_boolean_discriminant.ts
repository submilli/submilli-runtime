// A switch over a `boolean` field, or a plain `boolean`, that has a case for
// both `true` and `false` covers every value, even when one variant declares
// the field as `boolean` rather than a literal.
type Outcome = { ok: true; value: number } | { ok: boolean; error: string };
type Mixed = { ok: true; value: number } | { ok: false; error: string } | { ok: boolean; note: string };

function describeOutcome(outcome: Outcome): string {
  switch (outcome.ok) {
    case true: return "ok";
    case false: return "failed";
  }
}

function describeMixed(mixed: Mixed): string {
  switch (mixed.ok) {
    case true: return "ok";
    case false: return "failed";
  }
}

function describeFlag(flag: boolean): string {
  switch (flag) {
    case true: return "on";
    case false: return "off";
  }
}

function main(): void {
  console.log(describeOutcome({ ok: false, error: "e" }), describeOutcome({ ok: true, value: 1 }));
  console.log(describeMixed({ ok: true, note: "" }), describeMixed({ ok: false, error: "e" }));
  console.log(describeFlag(false), describeFlag(true));
}
