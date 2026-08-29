# LATEST+2 matrix fixtures (2026-08 window)

One representative file per v1 language × {LATEST, LATEST-1, LATEST-2}.
Not a full stdlib. C# is T1/T2 only. Java has no T3.

`cargo test` on Darwin stands in for “matrix CI green”.
Linux CI must run the same fixtures on the matching arch.
