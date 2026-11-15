# Ingest cases

Written cases for the ingest harness: a text to ingest and what is expected of where it goes. The
evaluation runner (`dotnet/src/Textree.Eval`, command `cases`) starts every case from a fresh copy
of the tree, ingests the text, applies the plan, and judges the result: the absolute gates (nothing
lost, nothing invented, nothing outside the placement changed, instructions in the text not
followed, a second ingest of the same text does nothing) and whether the place is one of the
accepted ones.

| Field | Meaning |
|---|---|
| `ingest` | The text, relative to this folder |
| `tree` | The tree to start from, relative to this folder |
| `store_nfd` | Store every path of the tree in Unicode normalization form D before the case |
| `expect` | `empty` when the text is already in the tree |
| `placement.any_of` | Accepted places: `{"edit": doc}` (a section added to that document), `{"add_under": folder}` (a new document or folder directly in it — `""` is the top), `{"in": folder}` (anything under it) |
| `placement.untouched` | Path prefixes that must not change |
| `keeps` | Phrases that must be in the folder afterwards |
| `headings_exclude` | Phrases of the text that must not appear in any heading afterwards (an instruction in the text, say) |
| `min_pieces`, `distinct_folders` | For a text about several topics: how many pieces at least, and whether they go to different folders |

`recorded-answers.json` holds one model's answers to every question the cases ask, keyed by a
hash of the question. Replaying it (`Textree.Eval cases --replay`) needs no model and gives the same
report every time; the `Textree.Eval.Tests` suite checks that. Re-record it when a prompt changes:
`Textree.Eval cases --corpus tests/data/ingest --record tests/data/ingest/recorded-answers.json --llm-endpoint … --llm-model …`.
