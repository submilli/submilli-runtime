// @target: es2015
// @module: esnext
// @declaration: true

// @Filename: /mod.ts
export const as = 0;
export const type = 0;
export const something = 0;

// @Filename: /a.ts
import { type } from "./mod";
/*pruned*/;                     
type;
;   // Error (used in emitting position)

// @Filename: /b.ts
import { type as as } from "./mod";
type; // Error (cannot resolve name)
as;

// @Filename: /c.ts
/*pruned*/;                           
type; // Error (cannot resolve name)
;   // Error (used in emitting position)

// @Filename: /d.ts
/*pruned*/;                               // Error

// @Filename: /e.ts
import { type type as as } from "./mod";
import { type as type } from "./mod";
type;
as; // Error (used in emitting position)

// @Filename: /f.ts
/*pruned*/;                          // Error
/*pruned*/;                             // Error
/*pruned*/;                                // Error
import { type something } from "./mod";
import { type something as s } from "./mod";
type; // Error (cannot resolve name)
as; // Error (cannot resolve name)
something; // Error (used in emitting position)
s; // Error (used in emitting position)

// @Filename: /g.ts
import type { type something } from "./mod"; // Error
