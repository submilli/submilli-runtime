// test262: test/built-ins/decodeURI/S15.1.3.1_A3_T2.js

function main(): void {
  assertSameValue(decodeURI("%3b"), "%3b", "#1: decodeURI(\"%3b\") equal \"%3b\", not \";\" or \"%3B\"");
  assertSameValue(decodeURI("%2f"), "%2f", "#2: decodeURI(\"%2f\") equal \"%2f\", not \"/\" or \"%2F\"");
  assertSameValue(decodeURI("%3f"), "%3f", "#3: decodeURI(\"%3f\") equal \"%3f\", not \"?\" or \"%3F\"");
  assertSameValue(decodeURI("%3a"), "%3a", "#4: decodeURI(\"%3a\") equal \"%3a\", not \":\" or \"%3A\"");
  assertSameValue(decodeURI("%40"), "%40", "#5: decodeURI(\"%40\") equal \"%40\", not \"@\"");
  assertSameValue(decodeURI("%26"), "%26", "#6: decodeURI(\"%26\") equal \"%26\", not \"&\"");
  assertSameValue(decodeURI("%3d"), "%3d", "#7.1: decodeURI(\"%3d\") equal \"%3d\", not \"=\" or \"%3D\"");
  assertSameValue(decodeURI("%2b"), "%2b", "#8.1: decodeURI(\"%2b\") equal \"%2b\", not \"+\" or \"%2B\"");
  assertSameValue(decodeURI("%24"), "%24", "#9: decodeURI(\"%24\") equal \"%24\", not \"$\"");
  assertSameValue(decodeURI("%2c"), "%2c", "#10: decodeURI(\"%2c\") equal \"%2c\", not \",\" or \"%2C\"");
}
