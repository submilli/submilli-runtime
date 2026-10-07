// test262: test/built-ins/decodeURI/S15.1.3.1_A1.1_T1.js

function main(): void {
  let result = true;
  let threw = false;
  try {
    decodeURI("%");
  } catch (e) {
    threw = e instanceof URIError;
  }
  if (!threw) {
    result = false;
  }
  let threw2 = false;
  try {
    decodeURI("%A");
  } catch (e) {
    threw2 = e instanceof URIError;
  }
  if (!threw2) {
    result = false;
  }
  let threw3 = false;
  try {
    decodeURI("%1");
  } catch (e) {
    threw3 = e instanceof URIError;
  }
  if (!threw3) {
    result = false;
  }
  let threw4 = false;
  try {
    decodeURI("% ");
  } catch (e) {
    threw4 = e instanceof URIError;
  }
  if (!threw4) {
    result = false;
  }
  assert(result === true, "#1: If string.charAt(k) equal \"%\" and k + 2 >= string.length, throw URIError");
}
