// Parameter types that refer back to themselves don't merge into one
// combined parameter, so the union is reported as not callable rather than
// merged forever.
// expect-error: cannot call value of type
// expect-error-count: 1
interface Emp {
  name: string;
  boss: Emp;
}

interface Dept {
  title: string;
  boss: Dept;
}

class Org {
  name: string = "acme";
  title: string = "root";
  boss: Org;
  constructor() {
    this.boss = this;
  }
}

const showEmp = (e: Emp): string => e.name;
const showDept = (d: Dept): string => d.title;

function main(): void {
  const show = Math.random() < 2 ? showEmp : showDept;
  console.log(show(new Org()));
}
