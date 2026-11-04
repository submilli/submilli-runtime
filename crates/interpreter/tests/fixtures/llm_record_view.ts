// A completion is an ordinary object. A program inspecting a value whose shape
// it is not sure of views it as `Record<string, unknown>`, enumerates it with
// `Object.keys`, or casts it to a declared subset of its fields — and all of
// that works on a completion as it does on an object literal, because a
// completion *is* one: the host builds it as a plain shape, not as an opaque
// host value with getters. The cast hands back the same object, not a copy.
// (Submilli accepts the direct `as Record<string, unknown>` from the declared
// interface; TypeScript needs `as unknown as` there, since an interface has no
// index signature. The mismatched-value case below goes through `unknown`.)
import llm from "submilli:llm";

interface Brief {
  ok: boolean;
  inputTokens?: number;
}

type Loose = Record<string, unknown>;

function main(): void {
  const raw = llm.texts("claude-haiku-4-5", ["Reply with the single word ok."]);
  const item = raw[0] as Record<string, unknown>;

  // The view enumerates exactly the fields the object carries: an absent
  // optional is an absent key, so `Object.keys` lists what `JSON.stringify` writes.
  assert(Object.keys(item).join(",") === "ok,value", "a record view lists the completion's own fields");
  assert(Object.values(item).length === 2, "and their values");
  assert(Object.entries(item)[0][0] === "ok", "entries pair each key with its value");

  // Indexed reads go through the same fields.
  assert(item["ok"] === true, "an indexed read sees the discriminant");
  assert(typeof item["value"] === "string", "and the text");
  assert(item["reason"] === undefined, "a field the arm does not carry reads as undefined");
  assert(!("inputTokens" in item), "an absent optional is not an own property");

  // The cast is a view, not a snapshot: it is the same object.
  assert((item as unknown) === (raw[0] as unknown), "the record view is the completion itself");

  // Through `unknown` and through an alias, the same.
  const anything: unknown = raw[0];
  const viaUnknown = anything as Record<string, unknown>;
  assert(viaUnknown["ok"] === true, "a cast through unknown checks the same shape");
  const loose = raw[0] as Loose;
  assert(loose["ok"] === true, "a cast through an alias checks the same shape");

  // A Record whose value type the fields do not fit is refused at runtime,
  // naming the key.
  let refused = "";
  try {
    const numbers = raw[0] as unknown as Record<string, number>;
    assert(numbers["ok"] === 1, "unreachable");
  } catch (e: TypeError) {
    refused = e.message;
  }
  assert(refused.indexOf("type mismatch") >= 0, "a mismatched Record value type is a TypeError");
  assert(refused.indexOf("ok") >= 0, "that names the offending key");

  // A structural cast to a declared subset of the fields passes, as it would
  // for any object of that shape.
  const brief = raw as Brief[];
  assert(brief[0].ok, "a subset interface reads the shared field");
  assert(brief[0].inputTokens === undefined, "and an absent optional stays absent");

  // JSON sees the same object.
  assert(JSON.stringify(raw[0]) === JSON.stringify(item), "the view stringifies as the completion does");
}
