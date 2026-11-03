// The untyped `call` returns a `Completion` envelope rather than a bare string,
// because a model call has more outcomes than "worked" and "threw". `ok` means
// the model stopped naturally — not merely that nothing was raised — and the
// envelope is a discriminated union, so a program cannot read `value` until it
// has checked `ok`. This fixture pins the shape of the success arm.
import llm from "submilli:llm";

function main(): void {
  const c = llm.call("claude-haiku-4-5", "Summarize this.");

  // A clean completion reports a natural stop and carries its text in `value`.
  assert(c.ok, "a clean completion reports a natural stop");
  if (!c.ok) return;
  assert(c.value.length > 0, "the completion text reaches the guest");

  // Usage is optional on both arms: this provider reported none, and absent
  // means indeterminate rather than free.
  assert(c.inputTokens === undefined, "unreported usage stays undefined rather than zero");
  assert(c.outputTokens === undefined, "unreported output usage stays undefined too");

  // The failure fields are not on the success arm at all, and an absent
  // optional is an absent key: the object carries exactly what it says.
  assert(Object.keys(c).join(",") === "ok,value", "the success arm carries only its own fields");
}
