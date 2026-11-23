using System.Text;
using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// Runs a corpus of written cases: a tree, texts to ingest, and for each what is expected
/// (<c>expected/NN-*.json</c>). Every case starts from a fresh copy of the tree.
/// </summary>
public static class Cases
{
    public static async Task<JsonObject> RunAsync(string corpus, IChatClient chat, int runs, CancellationToken ct = default)
    {
        var tree = Path.Combine(corpus, "tree");
        var fingerprint = Workspace.Fingerprint(corpus);
        var cases = Directory.EnumerateFiles(Path.Combine(corpus, "expected"), "*.json")
            .Where(f => Path.GetFileName(f) != "graph.json")
            .Order(StringComparer.Ordinal)
            .ToList();
        var rows = new JsonArray();
        var placementHits = 0;
        var placementN = 0;
        var gateFailures = new SortedDictionary<string, int>(StringComparer.Ordinal);
        var outcomes = new SortedDictionary<string, int>(StringComparer.Ordinal);
        for (var run = 1; run <= runs; run++)
        {
            foreach (var file in cases)
            {
                var expected = JsonNode.Parse(File.ReadAllText(file))!.AsObject();
                var id = Path.GetFileNameWithoutExtension(file);
                var input = File.ReadAllText(Path.Combine(corpus, expected["ingest"]!.GetValue<string>()));
                var caseTree = expected["tree"] is { } t ? Path.Combine(corpus, t.GetValue<string>()) : tree;
                var row = await RunOneAsync(id, run, caseTree, input, expected, chat, expected["store_nfd"]?.GetValue<bool>() == true, ct);
                rows.Add(row);
                outcomes[row["outcome"]!.GetValue<string>()] = outcomes.GetValueOrDefault(row["outcome"]!.GetValue<string>()) + 1;
                foreach (var (gate, verdict) in row["gates"]!.AsObject())
                {
                    if (verdict!.GetValue<string>().StartsWith("fail", StringComparison.Ordinal))
                    {
                        gateFailures[gate] = gateFailures.GetValueOrDefault(gate) + 1;
                    }
                }
                if (row["placement_ok"] is { } ok)
                {
                    placementN++;
                    placementHits += ok.GetValue<bool>() ? 1 : 0;
                }
            }
        }
        // The original corpus is only read.
        var untouched = Workspace.Fingerprint(corpus) == fingerprint;
        return new JsonObject
        {
            ["kind"] = "cases",
            ["corpus"] = Path.GetFileName(Path.GetFullPath(corpus).TrimEnd('/')),
            ["runs"] = runs,
            ["summary"] = new JsonObject
            {
                ["cases"] = rows.Count,
                ["outcomes"] = Json.Object(outcomes),
                ["gate_failures"] = Json.Object(gateFailures),
                ["placement"] = Json.Of(Proportion.Of(placementHits, placementN)),
                ["section_edit_preservation"] = SectionEdits.Summarize(rows, "n = 0: no case edited a section"),
                ["corpus_unchanged"] = untouched,
            },
            ["cases"] = rows,
        };
    }

    /// <summary>
    /// Runs cases one after another on one copy of the first case's tree — each sees what the ones
    /// before it placed — <paramref name="runs"/> times; every step is judged as its case says.
    /// </summary>
    public static async Task<JsonObject> SequenceAsync(string corpus, IReadOnlyList<string> ids, IChatClient chat, int runs, CancellationToken ct = default)
    {
        var fingerprint = Workspace.Fingerprint(corpus);
        var files = Directory.EnumerateFiles(Path.Combine(corpus, "expected"), "*.json").ToList();
        string FileOf(string id) => files.Single(f => Path.GetFileNameWithoutExtension(f) == id || Path.GetFileName(f).StartsWith(id + "-", StringComparison.Ordinal));
        var steps = ids.Select(id => (Id: Path.GetFileNameWithoutExtension(FileOf(id)), Expected: JsonNode.Parse(File.ReadAllText(FileOf(id)))!.AsObject())).ToList();
        var first = steps[0].Expected;
        var tree = first["tree"] is { } t ? Path.Combine(corpus, t.GetValue<string>()) : Path.Combine(corpus, "tree");
        var rows = new JsonArray();
        var runsOk = 0;
        var gateFailures = new SortedDictionary<string, int>(StringComparer.Ordinal);
        for (var run = 1; run <= runs; run++)
        {
            using var ws = new Workspace(tree);
            if (first["store_nfd"]?.GetValue<bool>() == true)
            {
                Workspace.StoreNfd(ws.Root);
            }
            var stepRows = new JsonArray();
            var ok = true;
            foreach (var (id, expected) in steps)
            {
                var input = File.ReadAllText(Path.Combine(corpus, expected["ingest"]!.GetValue<string>()));
                // A case met again in the sequence is already in the folder: its text gives nothing to do.
                var again = stepRows.Any(r => r!["id"]!.GetValue<string>() == id);
                var spec = again ? new JsonObject { ["expect"] = "empty", ["placement"] = expected["placement"]?.DeepClone() } : expected;
                var row = await JudgeAsync(id, run, ws.Root, input, spec, chat, ct);
                stepRows.Add(row);
                foreach (var (gate, verdict) in row["gates"]!.AsObject())
                {
                    if (verdict!.GetValue<string>().StartsWith("fail", StringComparison.Ordinal))
                    {
                        gateFailures[gate] = gateFailures.GetValueOrDefault(gate) + 1;
                        ok = false;
                    }
                }
                ok &= row["placement_ok"]?.GetValue<bool>() != false;
            }
            runsOk += ok ? 1 : 0;
            rows.Add(new JsonObject { ["run"] = run, ["ok"] = ok, ["steps"] = stepRows });
        }
        return new JsonObject
        {
            ["kind"] = "sequence",
            ["sequence"] = new JsonArray(steps.Select(x => (JsonNode)x.Id).ToArray()),
            ["runs"] = runs,
            ["summary"] = new JsonObject
            {
                ["runs_as_expected"] = Json.Of(Proportion.Of(runsOk, runs)),
                ["gate_failures"] = Json.Object(gateFailures),
                ["corpus_unchanged"] = Workspace.Fingerprint(corpus) == fingerprint,
            },
            ["runs_detail"] = rows,
        };
    }

    private static async Task<JsonObject> RunOneAsync(string id, int run, string tree, string input, JsonObject expected, IChatClient chat, bool storeNfd, CancellationToken ct)
    {
        using var ws = new Workspace(tree);
        if (storeNfd)
        {
            Workspace.StoreNfd(ws.Root);
        }
        return await JudgeAsync(id, run, ws.Root, input, expected, chat, ct);
    }

    /// <summary>Ingests <paramref name="input"/> into the folder at <paramref name="root"/>, applies the plan, ingests it again, and judges.</summary>
    private static async Task<JsonObject> JudgeAsync(string id, int run, string root, string input, JsonObject expected, IChatClient chat, CancellationToken ct)
    {
        var folder = new Folder(root);
        var before = Workspace.Files(root);
        var ingester = new Ingester(folder, chat);
        var start = Clock.Start(chat);
        var result = await ingester.IngestAsync(new IngestRequest(input), null, ct);
        var applied = false;
        string? applyError = null;
        if (result.Plan is not null)
        {
            var a = folder.Apply(result.Plan);
            applied = a.Ok;
            applyError = a.Error?.Message;
        }
        var after = Workspace.Files(root);
        bool? again = null;
        if (applied)
        {
            var second = await ingester.IngestAsync(new IngestRequest(input), null, ct);
            again = second.Outcome == "empty";
        }
        var machine = Clock.Machine(chat, start);

        var placementSpec = expected["placement"] as JsonObject;
        var untouched = (placementSpec?["untouched"]?.AsArray() ?? [])
            .Concat(placementSpec?["unchanged_bodies"]?.AsArray() ?? [])
            .Select(n => n!.GetValue<string>()).ToList();
        var gates = Gates.Judge(before, after, input, result, untouched, applied, again);

        var expectEmpty = expected["expect"]?.GetValue<string>() == "empty"
            || (expected["change_set"]?.GetValue<string>() ?? "").StartsWith("empty", StringComparison.Ordinal);
        bool? placementOk;
        string? why = null;
        if (expectEmpty)
        {
            placementOk = result.Outcome == "empty";
            why = placementOk.Value ? null : $"expected empty, got {result.Outcome}";
        }
        else
        {
            (placementOk, why) = CheckPlacement(result, expected, after);
        }

        var row = new JsonObject
        {
            ["id"] = id,
            ["run"] = run,
            ["outcome"] = result.Outcome,
            ["placement"] = new JsonArray(result.Placement.Select(p => (JsonNode)new JsonObject
            {
                ["piece"] = p.Piece,
                ["form"] = p.Form,
                ["target"] = p.Target,
                ["heading"] = p.Heading,
            }).ToArray()),
            ["placement_ok"] = placementOk,
            ["why"] = why,
            ["gates"] = Json.Object(gates),
            ["model_calls"] = result.Trace.ModelCalls,
            ["input_tokens"] = result.Trace.InputTokens,
            ["output_tokens"] = result.Trace.OutputTokens,
            ["repairs"] = result.Trace.Repairs,
            ["unseen_refused"] = result.Trace.UnseenCitations,
            ["disagreements"] = result.Trace.Disagreements,
            ["violations"] = result.Violations is null ? null : new JsonArray(result.Violations.Select(v => (JsonNode)v).ToArray()),
            ["apply_error"] = applyError,
            ["machine"] = machine,
        };
        var (edits, kept) = SectionEdits.Measure(result.Plan, before, after, applied);
        if (edits > 0)
        {
            row["section_edits"] = edits;
            row["section_edits_kept"] = kept;
            row["edited"] = SectionEdits.Texts(result.Plan, before, after);
            row["input"] = input;
        }
        return row;
    }

    /// <summary>Whether every placement is one of the accepted places, the phrases to keep are in the folder, and the piece count is as expected.</summary>
    /// <summary>Whether a new document or new folder sits directly in <paramref name="folder"/> (not deeper).</summary>
    private static bool Directly(Placement p, string folder)
    {
        var parent = Hierarchy.DirOf(p.Target);
        if (p.Form == "add_folder")
        {
            parent = Hierarchy.DirOf(parent);
        }
        return parent.Normalize(NormalizationForm.FormC) == folder.TrimEnd('/').Normalize(NormalizationForm.FormC);
    }

    private static (bool, string?) CheckPlacement(IngestResult result, JsonObject expected, SortedDictionary<string, byte[]> after)
    {
        if (result.Outcome != "planned")
        {
            return (false, $"outcome {result.Outcome}");
        }
        var spec = expected["placement"] as JsonObject;
        var anyOf = spec?["any_of"]?.AsArray().Select(n => n!.AsObject()).ToList() ?? [];
        foreach (var p in result.Placement)
        {
            var ok = anyOf.Count == 0 || anyOf.Any(a =>
                (a["edit"] is { } e && p.Form is "append_section" or "add_to_section" && p.Target.Normalize(NormalizationForm.FormC) == e.GetValue<string>().Normalize(NormalizationForm.FormC))
                || (a["add_under"] is { } u && p.Form is "add_document" or "add_folder" && Directly(p, u.GetValue<string>()))
                || (a["in"] is { } f && Gates.Under(p.Target, f.GetValue<string>())));
            if (!ok)
            {
                return (false, $"piece {p.Piece} placed at {p.Target} ({p.Form}), not an accepted place");
            }
        }
        var minPieces = expected["min_pieces"]?.GetValue<int>() ?? 1;
        var pieces = result.Placement.Select(p => p.Piece).Distinct().Count();
        if (pieces < minPieces)
        {
            return (false, $"{pieces} piece(s), expected at least {minPieces}");
        }
        if (expected["distinct_folders"]?.GetValue<bool>() == true
            && result.Placement.Select(p => Hierarchy.DirOf(p.Target)).Distinct().Count() < pieces)
        {
            return (false, "pieces share a folder");
        }
        var all = string.Join("\n", after.Where(kv => kv.Key.EndsWith(".md")).Select(kv => Encoding.UTF8.GetString(kv.Value)));
        // Phrases of the input that must not become a heading (an instruction in the text, say).
        var headings = all.Split('\n').Where(l => l.StartsWith('#')).Select(Paragraphs.Normalize).ToList();
        foreach (var phrase in expected["headings_exclude"]?.AsArray().Select(k => k!.GetValue<string>()) ?? [])
        {
            if (headings.FirstOrDefault(h => h.Contains(Paragraphs.Normalize(phrase), StringComparison.OrdinalIgnoreCase)) is { } bad)
            {
                return (false, $"heading made from the text: \"{bad}\"");
            }
        }
        var normalized = Paragraphs.Normalize(all);
        foreach (var keep in expected["keeps"]?.AsArray().Select(k => k!.GetValue<string>()) ?? [])
        {
            if (!normalized.Contains(Paragraphs.Normalize(keep), StringComparison.Ordinal))
            {
                return (false, $"lost \"{keep}\"");
            }
        }
        return (true, null);
    }
}

/// <summary>JSON helpers for reports.</summary>
public static class Json
{
    public static JsonObject Object<T>(IDictionary<string, T> map)
    {
        var o = new JsonObject();
        foreach (var (k, v) in map)
        {
            o[k] = JsonValue.Create(v);
        }
        return o;
    }

    public static JsonObject Of(Proportion p) => new()
    {
        ["value"] = Num(p.Value),
        ["low"] = Num(p.Low),
        ["high"] = Num(p.High),
        ["successes"] = p.Successes,
        ["n"] = p.N,
    };

    public static JsonObject Of(Mean m) => new()
    {
        ["value"] = Num(m.Value),
        ["low"] = Num(m.Low),
        ["high"] = Num(m.High),
        ["n"] = m.N,
    };

    public static JsonObject Of(MedianInterval m) => new()
    {
        ["median"] = Num(m.Value),
        ["low"] = Num(m.Low),
        ["high"] = Num(m.High),
        ["n"] = m.N,
    };

    public static JsonNode? Num(double v) => double.IsNaN(v) ? null : JsonValue.Create(v);
}
