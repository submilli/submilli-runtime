// @target: es2015
// @module: esnext, es2015, commonjs, amd, system, umd
// @filename: 0.ts
// @declaration: true
// @esModuleInterop: true
export const a = 1;
export const b = 2;

// @filename: 1.ts
/*pruned*/;               
ns.a;
ns.b;
let ns = {a: 1, b: 2}
ns.a;
ns.b;

// @filename: 2.ts
/*pruned*/;               

/**/;    
/**/;    