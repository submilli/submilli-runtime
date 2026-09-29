// test262: test/built-ins/decodeURI/S15.1.3.1_A1.1_T1.js
// URIError is erased to the base Error (README: no error subclasses); the
// "throws URIError" checks become "throws".

function main(): void {
  let result = true;
  let threw = false;
  try {
    decodeURI("%");
  } catch (e: Error) {
    threw = true;
  }
  if (!threw) {
    result = false;
  }
  let threw2 = false;
  try {
    decodeURI("%A");
  } catch (e: Error) {
    threw2 = true;
  }
  if (!threw2) {
    result = false;
  }
  let threw3 = false;
  try {
    decodeURI("%1");
  } catch (e: Error) {
    threw3 = true;
  }
  if (!threw3) {
    result = false;
  }
  let threw4 = false;
  try {
    decodeURI("% ");
  } catch (e: Error) {
    threw4 = true;
  }
  if (!threw4) {
    result = false;
  }
  assert(result === true, "#1: If string.charAt(k) equal \"%\" and k + 2 >= string.length, throw URIError");
}
