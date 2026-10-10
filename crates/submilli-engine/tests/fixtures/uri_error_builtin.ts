// A malformed `%` escape, or a lone surrogate given to an encoder, throws the
// built-in `URIError`, as in JavaScript.
function nameOf(run: () => string): string {
  try {
    run();
    return "none";
  } catch (e) {
    return e instanceof URIError ? `URIError:${e.message}` : e instanceof Error ? e.name : "?";
  }
}

function main(): void {
  assert(nameOf(() => decodeURIComponent("%")) === "URIError:URI malformed", "a truncated escape");
  assert(nameOf(() => decodeURI("%E0%A4%A")) === "URIError:URI malformed", "a truncated sequence");
  assert(nameOf(() => decodeURIComponent("%C0%80")) === "URIError:URI malformed", "an overlong form");
  assert(nameOf(() => encodeURIComponent(String.fromCharCode(0xd800))) === "URIError:URI malformed", "a lone high surrogate");
  assert(nameOf(() => encodeURI(`a${String.fromCharCode(0xdc00)}b`)) === "URIError:URI malformed", "a lone low surrogate");
  assert(encodeURIComponent("😀") === "%F0%9F%98%80", "a surrogate pair still encodes");

  const e = new URIError("custom");
  assert(e.name === "URIError" && e.message === "custom", "constructible");
  assert(e instanceof Error, "a URIError is an Error");
  assert(e.toString() === "URIError: custom", "toString");

  let caught = "";
  try {
    decodeURI("%zz");
  } catch (err) {
    if (err instanceof URIError) caught = err.name;
  }
  assert(caught === "URIError", "a catch narrows to the URIError");
  console.log(nameOf(() => decodeURIComponent("%")), nameOf(() => encodeURI(String.fromCharCode(0xd800))));
}
