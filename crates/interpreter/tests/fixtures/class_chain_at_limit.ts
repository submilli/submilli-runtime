// 62 classes, and 61 plus the library `Error`, are the longest chains a
// WebAssembly GC subtype hierarchy can hold.
class C0 { depth(): number { return 0; } }
class C1 extends C0 { depth(): number { return 1; } }
class C2 extends C1 { depth(): number { return 2; } }
class C3 extends C2 { depth(): number { return 3; } }
class C4 extends C3 { depth(): number { return 4; } }
class C5 extends C4 { depth(): number { return 5; } }
class C6 extends C5 { depth(): number { return 6; } }
class C7 extends C6 { depth(): number { return 7; } }
class C8 extends C7 { depth(): number { return 8; } }
class C9 extends C8 { depth(): number { return 9; } }
class C10 extends C9 { depth(): number { return 10; } }
class C11 extends C10 { depth(): number { return 11; } }
class C12 extends C11 { depth(): number { return 12; } }
class C13 extends C12 { depth(): number { return 13; } }
class C14 extends C13 { depth(): number { return 14; } }
class C15 extends C14 { depth(): number { return 15; } }
class C16 extends C15 { depth(): number { return 16; } }
class C17 extends C16 { depth(): number { return 17; } }
class C18 extends C17 { depth(): number { return 18; } }
class C19 extends C18 { depth(): number { return 19; } }
class C20 extends C19 { depth(): number { return 20; } }
class C21 extends C20 { depth(): number { return 21; } }
class C22 extends C21 { depth(): number { return 22; } }
class C23 extends C22 { depth(): number { return 23; } }
class C24 extends C23 { depth(): number { return 24; } }
class C25 extends C24 { depth(): number { return 25; } }
class C26 extends C25 { depth(): number { return 26; } }
class C27 extends C26 { depth(): number { return 27; } }
class C28 extends C27 { depth(): number { return 28; } }
class C29 extends C28 { depth(): number { return 29; } }
class C30 extends C29 { depth(): number { return 30; } }
class C31 extends C30 { depth(): number { return 31; } }
class C32 extends C31 { depth(): number { return 32; } }
class C33 extends C32 { depth(): number { return 33; } }
class C34 extends C33 { depth(): number { return 34; } }
class C35 extends C34 { depth(): number { return 35; } }
class C36 extends C35 { depth(): number { return 36; } }
class C37 extends C36 { depth(): number { return 37; } }
class C38 extends C37 { depth(): number { return 38; } }
class C39 extends C38 { depth(): number { return 39; } }
class C40 extends C39 { depth(): number { return 40; } }
class C41 extends C40 { depth(): number { return 41; } }
class C42 extends C41 { depth(): number { return 42; } }
class C43 extends C42 { depth(): number { return 43; } }
class C44 extends C43 { depth(): number { return 44; } }
class C45 extends C44 { depth(): number { return 45; } }
class C46 extends C45 { depth(): number { return 46; } }
class C47 extends C46 { depth(): number { return 47; } }
class C48 extends C47 { depth(): number { return 48; } }
class C49 extends C48 { depth(): number { return 49; } }
class C50 extends C49 { depth(): number { return 50; } }
class C51 extends C50 { depth(): number { return 51; } }
class C52 extends C51 { depth(): number { return 52; } }
class C53 extends C52 { depth(): number { return 53; } }
class C54 extends C53 { depth(): number { return 54; } }
class C55 extends C54 { depth(): number { return 55; } }
class C56 extends C55 { depth(): number { return 56; } }
class C57 extends C56 { depth(): number { return 57; } }
class C58 extends C57 { depth(): number { return 58; } }
class C59 extends C58 { depth(): number { return 59; } }
class C60 extends C59 { depth(): number { return 60; } }
class C61 extends C60 { depth(): number { return 61; } }
class E0 extends Error {}
class E1 extends E0 {}
class E2 extends E1 {}
class E3 extends E2 {}
class E4 extends E3 {}
class E5 extends E4 {}
class E6 extends E5 {}
class E7 extends E6 {}
class E8 extends E7 {}
class E9 extends E8 {}
class E10 extends E9 {}
class E11 extends E10 {}
class E12 extends E11 {}
class E13 extends E12 {}
class E14 extends E13 {}
class E15 extends E14 {}
class E16 extends E15 {}
class E17 extends E16 {}
class E18 extends E17 {}
class E19 extends E18 {}
class E20 extends E19 {}
class E21 extends E20 {}
class E22 extends E21 {}
class E23 extends E22 {}
class E24 extends E23 {}
class E25 extends E24 {}
class E26 extends E25 {}
class E27 extends E26 {}
class E28 extends E27 {}
class E29 extends E28 {}
class E30 extends E29 {}
class E31 extends E30 {}
class E32 extends E31 {}
class E33 extends E32 {}
class E34 extends E33 {}
class E35 extends E34 {}
class E36 extends E35 {}
class E37 extends E36 {}
class E38 extends E37 {}
class E39 extends E38 {}
class E40 extends E39 {}
class E41 extends E40 {}
class E42 extends E41 {}
class E43 extends E42 {}
class E44 extends E43 {}
class E45 extends E44 {}
class E46 extends E45 {}
class E47 extends E46 {}
class E48 extends E47 {}
class E49 extends E48 {}
class E50 extends E49 {}
class E51 extends E50 {}
class E52 extends E51 {}
class E53 extends E52 {}
class E54 extends E53 {}
class E55 extends E54 {}
class E56 extends E55 {}
class E57 extends E56 {}
class E58 extends E57 {}
class E59 extends E58 {}
class E60 extends E59 {}
function main(): void {
  const deepest: C0 = new C61();
  assert(deepest.depth() === 61, "dispatch through 62 classes");
  assert(new E60("deep") instanceof Error, "61 classes below Error");
}
