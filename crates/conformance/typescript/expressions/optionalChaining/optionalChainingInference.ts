// @target: es2015
// https://github.com/microsoft/TypeScript/issues/34579
/*pruned*/;                                                                           
const su: string | undefined = null as unknown as (string | undefined);
const fnu: (() => number) | undefined = null as unknown as ((() => number) | undefined);
const osu: { prop: string } | undefined = null as unknown as ({ prop: string } | undefined);
const ofnu: { prop: () => number } | undefined = null as unknown as ({ prop: () => number } | undefined);

const b1 = { value: su?.length };
/*pruned*/;                  

const b2 = { value: su?.length as number | undefined };
/*pruned*/;                  

const b3: { value: number | undefined } = { value: su?.length };
/*pruned*/;                  

const b4 = { value: fnu?.() };
/*pruned*/;                  

const b5 = { value: su?.["length"] };
/*pruned*/;                  

const b6 = { value: osu?.prop.length };
/*pruned*/;                  

const b7 = { value: osu?.prop["length"] };
/*pruned*/;                  

const b8 = { value: ofnu?.prop() };
/*pruned*/;                  



function main(): void {}
