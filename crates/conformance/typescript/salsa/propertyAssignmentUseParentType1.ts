// @target: es2015
// @strict: true
interface N {
    (): boolean
    num: 123;
}
export const interfaced: N = () => true;
interfaced.num = 123;

/*pruned*/;                                                  
/*pruned*/;       

export const ignoreJsdoc = () => true;
/** @type {string} make sure to ignore jsdoc! */
ignoreJsdoc.extra = 111


function main(): void {}
