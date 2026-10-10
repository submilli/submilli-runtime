import { Base } from "@t/lib";
export class Mid extends Base<number> {
  constructor(v: number) { super(v); }
  extra(): number { return 1; }
}
