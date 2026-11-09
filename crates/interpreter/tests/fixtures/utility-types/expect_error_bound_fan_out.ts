// Bounds that each name the next twice copy every bound per path. The copy
// stops at the type budget with a diagnostic instead of exhausting memory.
// expect-error: type is larger than the compiler limit of 65536 parts
function f<P0 extends { x: P1; y: P1 }, P1 extends { x: P2; y: P2 }, P2 extends { x: P3; y: P3 }, P3 extends { x: P4; y: P4 }, P4 extends { x: P5; y: P5 }, P5 extends { x: P6; y: P6 }, P6 extends { x: P7; y: P7 }, P7 extends { x: P8; y: P8 }, P8 extends { x: P9; y: P9 }, P9 extends { x: P10; y: P10 }, P10 extends { x: P11; y: P11 }, P11 extends { x: P12; y: P12 }, P12 extends { x: P13; y: P13 }, P13 extends { x: P14; y: P14 }, P14 extends { x: P15; y: P15 }, P15 extends { x: P16; y: P16 }, P16 extends { x: P17; y: P17 }, P17 extends { x: P18; y: P18 }, P18 extends { x: P19; y: P19 }, P19 extends { x: P20; y: P20 }, P20 extends { x: number; y: number }>(a: P0): keyof P0 { const k: keyof P0 = "x"; return k; }
function main(): void {}
