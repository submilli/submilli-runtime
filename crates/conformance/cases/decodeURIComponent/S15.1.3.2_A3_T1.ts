// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A3_T1.js

function main(): void {
  assertSameValue(decodeURIComponent("%3B"), ";", "#1: decodeURIComponent(\"%3B\") equal \";\", not \"%3B\"");
  assertSameValue(decodeURIComponent("%2F"), "/", "#2: decodeURIComponent(\"%2F\") equal \"/\", not \"%2F\"");
  assertSameValue(decodeURIComponent("%3F"), "?", "#3: decodeURIComponent(\"%3F\") equal \"?\", not \"%3F\"");
  assertSameValue(decodeURIComponent("%3A"), ":", "#4: decodeURIComponent(\"%3A\") equal \":\", not \"%3A\"");
  assertSameValue(decodeURIComponent("%40"), "@", "#5: decodeURIComponent(\"%40\") equal \"@\", not \"%40\"");
  assertSameValue(decodeURIComponent("%26"), "&", "#6: decodeURIComponent(\"%26\") equal \"&\", not \"%26\"");
  assertSameValue(decodeURIComponent("%3D"), "=", "#7.1: decodeURIComponent(\"%3D\") equal \"=\", not \"%3D\"");
  assertSameValue(decodeURIComponent("%2B"), "+", "#8.1: decodeURIComponent(\"%2B\") equal \"+\", not \"%2B\"");
  assertSameValue(decodeURIComponent("%24"), "$", "#9: decodeURIComponent(\"%24\") equal \"$\", not \"%24\"");
  assertSameValue(decodeURIComponent("%2C"), ",", "#10: decodeURIComponent(\"%2C\") equal \",\", not \"%2C\"");
  assertSameValue(decodeURIComponent("%23"), "#", "#11: decodeURIComponent(\"%23\") equal \"#\", not \"%23\"");
}
