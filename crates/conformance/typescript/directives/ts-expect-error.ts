// @target: es2015
// @ts-expect-error additional commenting
let invalidCommentedFancySingle: number = 'nope';

/*
 @ts-expect-error additional commenting */
let invalidCommentedFancyMulti: number = 'nope';

// @ts-expect-error additional commenting
let validCommentedFancySingle: string = 'nope';

/* @ts-expect-error additional commenting */
let validCommentedFancyMulti: string = 'nope';

// @ts-expect-error
let invalidCommentedPlainSingle: number = 'nope';

/*
 @ts-expect-error */
let invalidCommentedPlainMulti: number = 'nope';

// @ts-expect-error
let validCommentedPlainSingle: string = 'nope';

/* @ts-expect-error */
let validCommentedPlainMulti1: string = 'nope';

/*
@ts-expect-error */
let validCommentedPlainMulti2: string = 'nope';

let invalidPlain: number = 'nope';

let validPlain: string = 'nope';

/*pruned*/;        
/*pruned*/;                           // <-- compiles (as expected via comment)
/*pruned*/;                           // Should error

/*pruned*/;                           // error
/*pruned*/;                           // error

// @ts-expect-error: additional commenting with no whitespace
let invalidCommentedFancySingle_2: number = 'nope';

/*
 @ts-expect-error: additional commenting with no whitespace */
let invalidCommentedFancyMulti_2: number = 'nope';


function main(): void {}
