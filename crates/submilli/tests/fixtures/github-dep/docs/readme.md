# @demo/greeter

A tiny package that depends on `@submilli/greet` from
[`github.com/submilli/test-packages`](https://github.com/submilli/test-packages)
via a pinned GitHub dependency. It exists to exercise build-time
GitHub-dependency resolution end-to-end: `submilli build` fetches the dependency
at its pinned commit, installs it into the package store, and compiles this
package against it.
