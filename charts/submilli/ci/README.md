Values files consumed by `ct install`, which installs and tests the chart once
per file in this directory. They are not part of the packaged chart —
`.helmignore` excludes `/ci/`.

`seeded-values.yaml` needs a Secret named `submilli-ci-stripe` with an `api-key`
key to exist in the target namespace; the install job creates it.
