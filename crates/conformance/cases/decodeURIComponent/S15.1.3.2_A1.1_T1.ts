// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A1.1_T1.js

function main(): void {
  let result = true;
  let threw = false;
  try {
    decodeURIComponent("%");
  } catch (e) {
    threw = e instanceof URIError;
  }
  if (!threw) {
    result = false;
  }
  let threw2 = false;
  try {
    decodeURIComponent("%A");
  } catch (e) {
    threw2 = e instanceof URIError;
  }
  if (!threw2) {
    result = false;
  }
  let threw3 = false;
  try {
    decodeURIComponent("%1");
  } catch (e) {
    threw3 = e instanceof URIError;
  }
  if (!threw3) {
    result = false;
  }
  let threw4 = false;
  try {
    decodeURIComponent("% ");
  } catch (e) {
    threw4 = e instanceof URIError;
  }
  if (!threw4) {
    result = false;
  }
  assert(result === true, "#1: If string.charAt(k) equal \"%\" and k + 2 >= string.length, throw URIError");
}
