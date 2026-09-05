# diagnostics fixtures

Each `*.rsx` has a sibling `*.expected` with **all** the Outou diagnostics it must produce, verbatim, in the order the parser emits them. Diagnostics must be phrased in Outou vocabulary; a fixture that mentions the backend fails.
