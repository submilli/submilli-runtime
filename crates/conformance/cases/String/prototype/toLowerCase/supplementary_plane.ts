// test262: test/built-ins/String/prototype/toLowerCase/supplementary_plane.js

function main(): void {
  assertSameValue("\uD801\uDC00".toLowerCase(), "\uD801\uDC28", "DESERET CAPITAL LETTER LONG I");
  assertSameValue("\uD801\uDC01".toLowerCase(), "\uD801\uDC29", "DESERET CAPITAL LETTER LONG E");
  assertSameValue("\uD801\uDC02".toLowerCase(), "\uD801\uDC2A", "DESERET CAPITAL LETTER LONG A");
  assertSameValue("\uD801\uDC03".toLowerCase(), "\uD801\uDC2B", "DESERET CAPITAL LETTER LONG AH");
  assertSameValue("\uD801\uDC04".toLowerCase(), "\uD801\uDC2C", "DESERET CAPITAL LETTER LONG O");
  assertSameValue("\uD801\uDC05".toLowerCase(), "\uD801\uDC2D", "DESERET CAPITAL LETTER LONG OO");
  assertSameValue("\uD801\uDC06".toLowerCase(), "\uD801\uDC2E", "DESERET CAPITAL LETTER SHORT I");
  assertSameValue("\uD801\uDC07".toLowerCase(), "\uD801\uDC2F", "DESERET CAPITAL LETTER SHORT E");
  assertSameValue("\uD801\uDC08".toLowerCase(), "\uD801\uDC30", "DESERET CAPITAL LETTER SHORT A");
  assertSameValue("\uD801\uDC09".toLowerCase(), "\uD801\uDC31", "DESERET CAPITAL LETTER SHORT AH");
  assertSameValue("\uD801\uDC0A".toLowerCase(), "\uD801\uDC32", "DESERET CAPITAL LETTER SHORT O");
  assertSameValue("\uD801\uDC0B".toLowerCase(), "\uD801\uDC33", "DESERET CAPITAL LETTER SHORT OO");
  assertSameValue("\uD801\uDC0C".toLowerCase(), "\uD801\uDC34", "DESERET CAPITAL LETTER AY");
  assertSameValue("\uD801\uDC0D".toLowerCase(), "\uD801\uDC35", "DESERET CAPITAL LETTER OW");
  assertSameValue("\uD801\uDC0E".toLowerCase(), "\uD801\uDC36", "DESERET CAPITAL LETTER WU");
  assertSameValue("\uD801\uDC0F".toLowerCase(), "\uD801\uDC37", "DESERET CAPITAL LETTER YEE");
  assertSameValue("\uD801\uDC10".toLowerCase(), "\uD801\uDC38", "DESERET CAPITAL LETTER H");
  assertSameValue("\uD801\uDC11".toLowerCase(), "\uD801\uDC39", "DESERET CAPITAL LETTER PEE");
  assertSameValue("\uD801\uDC12".toLowerCase(), "\uD801\uDC3A", "DESERET CAPITAL LETTER BEE");
  assertSameValue("\uD801\uDC13".toLowerCase(), "\uD801\uDC3B", "DESERET CAPITAL LETTER TEE");
  assertSameValue("\uD801\uDC14".toLowerCase(), "\uD801\uDC3C", "DESERET CAPITAL LETTER DEE");
  assertSameValue("\uD801\uDC15".toLowerCase(), "\uD801\uDC3D", "DESERET CAPITAL LETTER CHEE");
  assertSameValue("\uD801\uDC16".toLowerCase(), "\uD801\uDC3E", "DESERET CAPITAL LETTER JEE");
  assertSameValue("\uD801\uDC17".toLowerCase(), "\uD801\uDC3F", "DESERET CAPITAL LETTER KAY");
  assertSameValue("\uD801\uDC18".toLowerCase(), "\uD801\uDC40", "DESERET CAPITAL LETTER GAY");
  assertSameValue("\uD801\uDC19".toLowerCase(), "\uD801\uDC41", "DESERET CAPITAL LETTER EF");
  assertSameValue("\uD801\uDC1A".toLowerCase(), "\uD801\uDC42", "DESERET CAPITAL LETTER VEE");
  assertSameValue("\uD801\uDC1B".toLowerCase(), "\uD801\uDC43", "DESERET CAPITAL LETTER ETH");
  assertSameValue("\uD801\uDC1C".toLowerCase(), "\uD801\uDC44", "DESERET CAPITAL LETTER THEE");
  assertSameValue("\uD801\uDC1D".toLowerCase(), "\uD801\uDC45", "DESERET CAPITAL LETTER ES");
  assertSameValue("\uD801\uDC1E".toLowerCase(), "\uD801\uDC46", "DESERET CAPITAL LETTER ZEE");
  assertSameValue("\uD801\uDC1F".toLowerCase(), "\uD801\uDC47", "DESERET CAPITAL LETTER ESH");
  assertSameValue("\uD801\uDC20".toLowerCase(), "\uD801\uDC48", "DESERET CAPITAL LETTER ZHEE");
  assertSameValue("\uD801\uDC21".toLowerCase(), "\uD801\uDC49", "DESERET CAPITAL LETTER ER");
  assertSameValue("\uD801\uDC22".toLowerCase(), "\uD801\uDC4A", "DESERET CAPITAL LETTER EL");
  assertSameValue("\uD801\uDC23".toLowerCase(), "\uD801\uDC4B", "DESERET CAPITAL LETTER EM");
  assertSameValue("\uD801\uDC24".toLowerCase(), "\uD801\uDC4C", "DESERET CAPITAL LETTER EN");
  assertSameValue("\uD801\uDC25".toLowerCase(), "\uD801\uDC4D", "DESERET CAPITAL LETTER ENG");
  assertSameValue("\uD801\uDC26".toLowerCase(), "\uD801\uDC4E", "DESERET CAPITAL LETTER OI");
  assertSameValue("\uD801\uDC27".toLowerCase(), "\uD801\uDC4F", "DESERET CAPITAL LETTER EW");
}
