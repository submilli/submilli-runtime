// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A3_T2.js

function main(): void {
  assertSameValue(decodeURIComponent("%3b"), ";", "#1: decodeURIComponent(\"%3b\") equal \";\", not \"%3B\" or \"%3b\"");
  assertSameValue(decodeURIComponent("%2f"), "/", "#2: decodeURIComponent(\"%2f\") equal \"/\", not \"%2F\" or \"%2f\"");
  assertSameValue(decodeURIComponent("%3f"), "?", "#3: decodeURIComponent(\"%3f\") equal \"?\", not \"%3F\" or \"%3f\"");
  assertSameValue(decodeURIComponent("%3a"), ":", "#4: decodeURIComponent(\"%3a\") equal \":\", not \"%3A\" or \"%3a\"");
  assertSameValue(decodeURIComponent("%40"), "@", "#5: decodeURIComponent(\"%40\") equal \"@\", not \"%40\"");
  assertSameValue(decodeURIComponent("%26"), "&", "#6: decodeURIComponent(\"%26\") equal \"&\", not \"%26\"");
  assertSameValue(decodeURIComponent("%3d"), "=", "#7.1: decodeURIComponent(\"%3d\") equal \"=\", not \"%3D\" or \"%3d\"");
  assertSameValue(decodeURIComponent("%2b"), "+", "#8.1: decodeURIComponent(\"%2b\") equal \"+\", not \"%2B\" or \"%2b\"");
  assertSameValue(decodeURIComponent("%24"), "$", "#9: decodeURIComponent(\"%24\") equal \"$\", not \"%24\"");
  assertSameValue(decodeURIComponent("%2c"), ",", "#10: decodeURIComponent(\"%2c\") equal \",\", not \"%2C\" or \"%2c\"");
  assertSameValue(decodeURIComponent("%23"), "#", "#11: decodeURIComponent(\"%23\") equal \"#\", not \"%23\"");
}
