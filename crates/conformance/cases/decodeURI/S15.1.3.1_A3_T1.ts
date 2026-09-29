// test262: test/built-ins/decodeURI/S15.1.3.1_A3_T1.js

function main(): void {
  assertSameValue(decodeURI("%3B"), "%3B", "#1: decodeURI(\"%3B\") equal \"%3B\", not \";\"");
  assertSameValue(decodeURI("%2F"), "%2F", "#2: decodeURI(\"%2F\") equal \"%2F\", not \"/\"");
  assertSameValue(decodeURI("%3F"), "%3F", "#3: decodeURI(\"%3F\") equal \"%3F\", not \"?\"");
  assertSameValue(decodeURI("%3A"), "%3A", "#4: decodeURI(\"%3A\") equal \"%3A\", not \":\"");
  assertSameValue(decodeURI("%40"), "%40", "#5: decodeURI(\"%40\") equal \"%40\", not \"@\"");
  assertSameValue(decodeURI("%26"), "%26", "#6: decodeURI(\"%26\") equal \"%26\", not \"&\"");
  assertSameValue(decodeURI("%3D"), "%3D", "#7.1: decodeURI(\"%3D\") equal \"%3D\", not \"=\"");
  assertSameValue(decodeURI("%2B"), "%2B", "#8.1: decodeURI(\"%2B\") equal \"%2B\", not \"+\"");
  assertSameValue(decodeURI("%24"), "%24", "#9: decodeURI(\"%24\") equal \"%24\", not \"$\"");
  assertSameValue(decodeURI("%2C"), "%2C", "#10: decodeURI(\"%2C\") equal \"%2C\", not \",\"");
}
