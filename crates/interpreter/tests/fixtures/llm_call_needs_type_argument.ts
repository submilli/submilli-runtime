// `call<T>` and `batch<T>` check the answer against `T`, so `T` is required:
// written, or supplied by the expected type. With neither, the call is an
// error that names the text form, whichever way the function was imported —
// never a silent `unknown` and never the text under the typed name.
// expect-error: `llm.call` needs its type argument
// expect-error: `llm.batch` needs its type argument
// expect-error: `llm.call` needs its type argument
import llm from "submilli:llm";
import { call } from "submilli:llm";

function main(): void {
  const a = llm.call("m", "p");
  const b = llm.batch("m", ["p"]);
  const c = call("m", "p");
  assert(a.ok && b.length === 1 && c.ok, "unreachable — the calls above do not compile");
}
