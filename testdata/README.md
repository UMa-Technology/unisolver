# Test data

Public tests generate their inputs (synthetic star fields from `unisolver-synth`)
or use the databases bundled with the Flutter plugin, so they run on a clean clone.

Maintainers additionally link a private corpus of real captures at
`testdata/private/` (not part of this repository). Tests that need it print a
`skipped: ...` line and pass when it is absent.
