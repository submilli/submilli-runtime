// test262: test/built-ins/String/prototype/toUpperCase/supplementary_plane.js

function main(): void {
  assertSameValue("\uD801\uDC28".toUpperCase(), "\uD801\uDC00", "DESERET SMALL LETTER LONG I");
  assertSameValue("\uD801\uDC29".toUpperCase(), "\uD801\uDC01", "DESERET SMALL LETTER LONG E");
  assertSameValue("\uD801\uDC2A".toUpperCase(), "\uD801\uDC02", "DESERET SMALL LETTER LONG A");
  assertSameValue("\uD801\uDC2B".toUpperCase(), "\uD801\uDC03", "DESERET SMALL LETTER LONG AH");
  assertSameValue("\uD801\uDC2C".toUpperCase(), "\uD801\uDC04", "DESERET SMALL LETTER LONG O");
  assertSameValue("\uD801\uDC2D".toUpperCase(), "\uD801\uDC05", "DESERET SMALL LETTER LONG OO");
  assertSameValue("\uD801\uDC2E".toUpperCase(), "\uD801\uDC06", "DESERET SMALL LETTER SHORT I");
  assertSameValue("\uD801\uDC2F".toUpperCase(), "\uD801\uDC07", "DESERET SMALL LETTER SHORT E");
  assertSameValue("\uD801\uDC30".toUpperCase(), "\uD801\uDC08", "DESERET SMALL LETTER SHORT A");
  assertSameValue("\uD801\uDC31".toUpperCase(), "\uD801\uDC09", "DESERET SMALL LETTER SHORT AH");
  assertSameValue("\uD801\uDC32".toUpperCase(), "\uD801\uDC0A", "DESERET SMALL LETTER SHORT O");
  assertSameValue("\uD801\uDC33".toUpperCase(), "\uD801\uDC0B", "DESERET SMALL LETTER SHORT OO");
  assertSameValue("\uD801\uDC34".toUpperCase(), "\uD801\uDC0C", "DESERET SMALL LETTER AY");
  assertSameValue("\uD801\uDC35".toUpperCase(), "\uD801\uDC0D", "DESERET SMALL LETTER OW");
  assertSameValue("\uD801\uDC36".toUpperCase(), "\uD801\uDC0E", "DESERET SMALL LETTER WU");
  assertSameValue("\uD801\uDC37".toUpperCase(), "\uD801\uDC0F", "DESERET SMALL LETTER YEE");
  assertSameValue("\uD801\uDC38".toUpperCase(), "\uD801\uDC10", "DESERET SMALL LETTER H");
  assertSameValue("\uD801\uDC39".toUpperCase(), "\uD801\uDC11", "DESERET SMALL LETTER PEE");
  assertSameValue("\uD801\uDC3A".toUpperCase(), "\uD801\uDC12", "DESERET SMALL LETTER BEE");
  assertSameValue("\uD801\uDC3B".toUpperCase(), "\uD801\uDC13", "DESERET SMALL LETTER TEE");
  assertSameValue("\uD801\uDC3C".toUpperCase(), "\uD801\uDC14", "DESERET SMALL LETTER DEE");
  assertSameValue("\uD801\uDC3D".toUpperCase(), "\uD801\uDC15", "DESERET SMALL LETTER CHEE");
  assertSameValue("\uD801\uDC3E".toUpperCase(), "\uD801\uDC16", "DESERET SMALL LETTER JEE");
  assertSameValue("\uD801\uDC3F".toUpperCase(), "\uD801\uDC17", "DESERET SMALL LETTER KAY");
  assertSameValue("\uD801\uDC40".toUpperCase(), "\uD801\uDC18", "DESERET SMALL LETTER GAY");
  assertSameValue("\uD801\uDC41".toUpperCase(), "\uD801\uDC19", "DESERET SMALL LETTER EF");
  assertSameValue("\uD801\uDC42".toUpperCase(), "\uD801\uDC1A", "DESERET SMALL LETTER VEE");
  assertSameValue("\uD801\uDC43".toUpperCase(), "\uD801\uDC1B", "DESERET SMALL LETTER ETH");
  assertSameValue("\uD801\uDC44".toUpperCase(), "\uD801\uDC1C", "DESERET SMALL LETTER THEE");
  assertSameValue("\uD801\uDC45".toUpperCase(), "\uD801\uDC1D", "DESERET SMALL LETTER ES");
  assertSameValue("\uD801\uDC46".toUpperCase(), "\uD801\uDC1E", "DESERET SMALL LETTER ZEE");
  assertSameValue("\uD801\uDC47".toUpperCase(), "\uD801\uDC1F", "DESERET SMALL LETTER ESH");
  assertSameValue("\uD801\uDC48".toUpperCase(), "\uD801\uDC20", "DESERET SMALL LETTER ZHEE");
  assertSameValue("\uD801\uDC49".toUpperCase(), "\uD801\uDC21", "DESERET SMALL LETTER ER");
  assertSameValue("\uD801\uDC4A".toUpperCase(), "\uD801\uDC22", "DESERET SMALL LETTER EL");
  assertSameValue("\uD801\uDC4B".toUpperCase(), "\uD801\uDC23", "DESERET SMALL LETTER EM");
  assertSameValue("\uD801\uDC4C".toUpperCase(), "\uD801\uDC24", "DESERET SMALL LETTER EN");
  assertSameValue("\uD801\uDC4D".toUpperCase(), "\uD801\uDC25", "DESERET SMALL LETTER ENG");
  assertSameValue("\uD801\uDC4E".toUpperCase(), "\uD801\uDC26", "DESERET SMALL LETTER OI");
  assertSameValue("\uD801\uDC4F".toUpperCase(), "\uD801\uDC27", "DESERET SMALL LETTER EW");
}
