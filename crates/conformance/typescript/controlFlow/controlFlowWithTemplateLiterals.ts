// @target: es2015
// @strictNullChecks: true
const envVar: string | null = null as unknown as (string | null);
if (typeof envVar === `string`) {
  envVar.slice(0)
}

const obj: {test: string} | {} = null as unknown as ({test: string} | {});
if (`test` in obj) {
  obj.test.slice(0)
}


function main(): void {}
