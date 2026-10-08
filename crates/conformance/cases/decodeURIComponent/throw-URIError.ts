// test262: test/built-ins/decodeURIComponent/throw-URIError.js

function throwsUriError(uri: string): boolean {
  try {
    decodeURIComponent(uri);
  } catch (e) {
    return e instanceof URIError;
  }
  return false;
}

function main(): void {
  assert(throwsUriError("%ED%BF%BF"), "#1: %ED%BF%BF (surrogate pair) should throw URIError");
  assert(throwsUriError("%C0%AF"), "#2: %C0%AF (overlong encoding) should throw URIError");
  assert(throwsUriError("%ED%7F%BF"), "#3: %ED%7F%BF (invalid continuation) should throw URIError");
  assert(throwsUriError("%ED%BF"), "#4: %ED%BF (incomplete sequence) should throw URIError");
  assert(throwsUriError("%F4%90%80%80"), "#5: %F4%90%80%80 (out-of-range) should throw URIError");
}
